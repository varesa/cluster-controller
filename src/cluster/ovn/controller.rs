use futures::StreamExt;
use kube::runtime::controller::{Context, Controller, ReconcilerAction};
use kube::{
    api::{Api, ListParams, PostParams},
    Client, ResourceExt,
};
use serde_json::json;
use tokio::time::Duration;

use crate::cluster::ovn::lowlevel::Ovn;
use crate::crd::libvirt::{NetworkAttachment, VirtualMachine};
use crate::crd::ovn::{
    DhcpOptions, Network, NetworkStatus, Route, Router, RouterAttachment, RouterStatus,
};
use crate::errors::Error;
use crate::utils::name_namespaced;
use crate::{
    api_replace_resource, client_ensure_finalizer, client_remove_finalizer, create_controller,
    create_set_status, ok_and_requeue, ok_no_requeue, resource_has_finalizer, GROUP_NAME,
};

/// State available for the reconcile and error_policy functions
/// called by the Controller
struct State {
    client: Client,
}

create_set_status!(Network, NetworkStatus, set_network_status);
create_set_status!(Router, RouterStatus, set_router_status);

fn ensure_network_exists(name: &str) {
    let mut ovn = Ovn::new("10.4.3.1", 6641);
    if ovn.get_ls(name).is_err() {
        println!("ovn: Sw {name} doesn't exist, creating");
        ovn.add_ls(name);
    }
}

fn ensure_router_exists(name: &str) {
    let mut ovn = Ovn::new("10.4.3.1", 6641);
    if ovn.get_lr(name).is_err() {
        println!("ovn: Router {name} doesn't exist, creating");
        ovn.add_lr(name);
    }
}

fn ensure_router_routes(router: &str, routes: &[Route]) -> Result<(), Error> {
    let mut ovn = Ovn::new("10.4.3.1", 6641);
    ovn.set_lr_routes(router, routes)?;
    Ok(())
}

fn ensure_dhcp(name: &str, dhcp: &DhcpOptions) -> Result<(), Error> {
    let mut ovn = Ovn::new("10.4.3.1", 6641);
    if ovn.get_dhcp_options(&dhcp.cidr).is_none() {
        ovn.create_dhcp_option_set(dhcp)?;
    }
    // Lazily try to always update, effectively noop if current value are already correct
    ovn.set_dhcp_option_set_options(dhcp)?;
    ovn.set_ls_cidr(name, &dhcp.cidr)?;
    Ok(())
}

fn ensure_router_attachment(
    network: &Network,
    router_attachment: &RouterAttachment,
) -> Result<(), Error> {
    let network_ns = ResourceExt::namespace(network).expect("Get network ns");

    let split: Vec<String> = router_attachment
        .name
        .split('/')
        .map(String::from)
        .collect();
    let (namespace, name) = match split.len() {
        1 => (&network_ns, split.get(0).unwrap()),
        2 => (split.get(0).unwrap(), split.get(1).unwrap()),
        _ => panic!("Malformed router name (todo: error)"),
    };

    let mut ovn = Ovn::new("10.4.3.1", 6641);
    let lr = ovn.get_lr(&format!("{}-{}", &namespace, &name))?;

    let ls_name = name_namespaced(network);
    ovn.get_ls(&ls_name)?;

    let lrp_name = format!("lr_{}-{}_ls_{}", namespace, name, name_namespaced(network));
    if ovn.get_lrp(&lrp_name).is_err() {
        ovn.add_lrp(&lr.name, &lrp_name, &router_attachment.address)?;
    } else {
        ovn.update_lrp(&lrp_name, &router_attachment.address)?;
    }

    let lsp_name = format!("ls_{}_lr_{}-{}", ls_name, namespace, name);
    let params = json!({
        "type": "router",
        "addresses": "router",
        "options": ["map", [ ["router-port", lrp_name] ]]
    });
    if ovn.get_lsp(&lsp_name).is_err() {
        ovn.add_lsp(&ls_name, &lsp_name, Some(params.as_object().unwrap()))?;
    }
    Ok(())
}

fn delete_network(name: &str) -> Result<(), Error> {
    let mut ovn = Ovn::new("10.4.3.1", 6641);
    ovn.del_ls_by_name(name)?;
    Ok(())
}

fn delete_router(name: &str) -> Result<(), Error> {
    let mut ovn = Ovn::new("10.4.3.1", 6641);
    ovn.del_lr_by_name(name)?;
    Ok(())
}

/// Handle updates to networks in the cluster
async fn reconcile_network(
    network: Network,
    ctx: Context<State>,
) -> Result<ReconcilerAction, Error> {
    let client = ctx.get_ref().client.clone();
    let name = name_namespaced(&network);

    if network.metadata.deletion_timestamp.is_some() {
        println!("ovn: Network {} waiting for deletion", name);
        delete_network(&name)?;
        client_remove_finalizer!(client, Network, &network, "ovn");
        println!("ovn: Network {} deleted", name);
    } else {
        println!("ovn: update for network {name}");
        client_ensure_finalizer!(client, Network, &network, "ovn");
        ensure_network_exists(&name);
        if let Some(dhcp_options) = network.spec.dhcp.as_ref() {
            ensure_dhcp(&name, dhcp_options)?;
        }

        if let Some(routers) = network.spec.routers.as_ref() {
            for router in routers {
                ensure_router_attachment(&network, router)?;
            }
        }

        println!("ovn: update for network {name} successful");

        let status = NetworkStatus { is_created: true };
        set_network_status(&network, status, client.clone()).await?;
    }

    ok_and_requeue!(600)
}

async fn connect_vm_nic(
    client: Client,
    vm: &VirtualMachine,
    nic: &NetworkAttachment,
) -> Result<(), Error> {
    let mut ovn = Ovn::new("10.4.3.1", 6641);
    let namespace = ResourceExt::namespace(vm).expect("Failed to get VM namespace");
    let network_name = nic.name.as_ref().expect("No network name set");
    let ls_name = format!("{}-{}", &namespace, &network_name);
    if ovn.get_lsp(nic.ovn_id.as_ref().unwrap()).is_err() {
        println!("ovn: lsp missing for NIC, creating");
        let lsp_id = nic.ovn_id.as_ref().unwrap();
        ovn.add_lsp(&ls_name, lsp_id, None)?;
        ovn.set_lsp_address(
            lsp_id,
            nic.mac_address.as_ref().expect("MAC address missing"),
        )?;

        let api: Api<Network> = Api::namespaced(client.clone(), &namespace);
        let network = api.get(network_name).await?;
        if let Some(dhcp) = network.spec.dhcp {
            ovn.set_lsp_dhcp_options(lsp_id, &dhcp.cidr)?;
        }
    }
    Ok(())
}

fn disconnect_vm_nic(vm: &VirtualMachine, nic: &NetworkAttachment) -> Result<(), Error> {
    let mut ovn = Ovn::new("10.4.3.1", 6641);
    let ls_name = format!(
        "{}-{}",
        ResourceExt::namespace(vm).expect("Failed to get VM namespace"),
        nic.name.as_ref().expect("No network name set")
    );
    if ovn.get_lsp(nic.ovn_id.as_ref().unwrap()).is_ok() {
        println!("ovn: lsp exists for NIC, removing");
        ovn.del_lsp(&ls_name, nic.ovn_id.as_ref().unwrap())?;
    }
    Ok(())
}

fn get_vm_ovn_nics(vm: &VirtualMachine) -> Vec<NetworkAttachment> {
    vm.spec
        .networks
        .clone()
        .into_iter()
        .filter(|net| net.ovn_id.is_some())
        .collect()
}

/// Handle updates to VMs in the cluster
async fn reconcile_vm(vm: VirtualMachine, ctx: Context<State>) -> Result<ReconcilerAction, Error> {
    let name = name_namespaced(&vm);
    let client = ctx.get_ref().client.clone();

    if vm.metadata.deletion_timestamp.is_some() {
        println!("ovn: VM {name} waiting for deletion");
        for (index, nic) in get_vm_ovn_nics(&vm).iter().enumerate() {
            println!("ovn: disconnecting NIC {index} for VM {name}");
            disconnect_vm_nic(&vm, nic)?;
        }
        client_remove_finalizer!(client, VirtualMachine, &vm, "ovn");
        ok_no_requeue!()
    } else {
        println!("ovn: VM {name} updated");
        client_ensure_finalizer!(client, VirtualMachine, &vm, "ovn");
        for (index, nic) in get_vm_ovn_nics(&vm).iter().enumerate() {
            println!("ovn: connecting NIC {index} for VM {name}");
            connect_vm_nic(client.clone(), &vm, nic).await?;
        }

        ok_and_requeue!(600)
    }
}

/// Handle updates to routers in the cluster
async fn reconcile_router(router: Router, ctx: Context<State>) -> Result<ReconcilerAction, Error> {
    let client = ctx.get_ref().client.clone();
    let name = name_namespaced(&router);

    if router.metadata.deletion_timestamp.is_some() {
        println!("ovn: Router {} waiting for deletion", name);
        delete_router(&name)?;
        client_remove_finalizer!(client, Router, &router, "ovn");
        println!("ovn: Router {} deleted", name);
    } else {
        println!("ovn: update for router {name}");
        client_ensure_finalizer!(client, Router, &router, "ovn");

        ensure_router_exists(&name);
        if let Some(routes) = &router.spec.routes {
            ensure_router_routes(&name, routes)?;
        } else {
            ensure_router_routes(&name, &[])?;
        }
        println!("ovn: update for router {name} successful");

        let status = RouterStatus { is_created: true };
        set_router_status(&router, status, client.clone()).await?;
    }

    ok_and_requeue!(600)
}

fn error_policy(_error: &Error, _ctx: Context<State>) -> ReconcilerAction {
    ReconcilerAction {
        requeue_after: Some(Duration::from_secs(15)),
    }
}

pub async fn create(client: Client) -> Result<(), Error> {
    let context = Context::new(State {
        client: client.clone(),
    });
    println!("ovn: Starting controllers");

    let networks: Api<Network> = Api::all(client.clone());
    let routers: Api<Router> = Api::all(client.clone());
    let vms: Api<VirtualMachine> = Api::all(client.clone());

    let context_clone = context.clone();
    let vm_task = tokio::task::spawn(async {
        panic!(
            "OVN VM-controller task exited: {:?}",
            create_controller!(vms, reconcile_vm, error_policy, context_clone)
        );
    });

    let context_clone = context.clone();
    let network_task = tokio::task::spawn(async {
        panic!(
            "OVN Network-controller task exited: {:?}",
            create_controller!(networks, reconcile_network, error_policy, context_clone)
        );
    });

    let router_task = tokio::task::spawn(async {
        panic!(
            "OVN Router-controller task exited: {:?}",
            create_controller!(routers, reconcile_router, error_policy, context)
        );
    });

    let _ = tokio::try_join!(vm_task, network_task, router_task);
    Ok(())
}
