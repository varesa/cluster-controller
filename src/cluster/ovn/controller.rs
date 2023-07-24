use std::sync::Arc;

use futures::StreamExt;
use kube::runtime::controller::{Action, Controller};
use kube::{
    api::{Api, PostParams},
    Client, Resource, ResourceExt,
};
use serde_json::json;
use tokio::time::Duration;

use super::common::OvnNamed;
use super::logicalswitch::LogicalSwitch;
use crate::cluster::ovn::{
    common::OvnBasicActions, common::OvnNamedGetters, dhcpoptions::DhcpOptions,
    logicalrouter::LogicalRouter, logicalswitchport::LogicalSwitchPort, lowlevel::Ovn,
};
use crate::crd::libvirt::{set_vm_status, NetworkAttachment, VirtualMachine, VirtualMachineStatus};
use crate::crd::ovn::{
    DhcpOptions as DhcpOptionsCrd, Network, NetworkStatus, Route, Router, RouterAttachment,
    RouterStatus,
};
use crate::errors::Error;
use crate::metadataservice::deployment::deploy as deploy_mds;
use crate::utils::{name_namespaced, ExtendResource};
use crate::{create_controller, create_set_status, ok_and_requeue, ok_no_requeue};

/// State available for the reconcile and error_policy functions
/// called by the Controller
struct State {
    client: Client,
}

create_set_status!(Network, NetworkStatus, set_network_status);
create_set_status!(Router, RouterStatus, set_router_status);

fn ensure_router_routes(router: &str, routes: &[Route]) -> Result<(), Error> {
    let ovn = Arc::new(Ovn::new("10.4.3.1", 6641));
    LogicalRouter::get_by_name(ovn, router)?.set_routes(routes)
}

fn ensure_dhcp(name: &str, dhcp: &DhcpOptionsCrd) -> Result<(), Error> {
    let ovn = Arc::new(Ovn::new("10.4.3.1", 6641));
    let mut dhcp_opts = match DhcpOptions::get_by_cidr(ovn.clone(), &dhcp.cidr) {
        Ok(opts) => Ok(opts),
        Err(Error::OvnNotFound(_, _)) => DhcpOptions::create(ovn, &dhcp.cidr),
        Err(e) => Err(e),
    }?;

    // Lazily try to always update, effectively noop if current value are already correct
    dhcp_opts.set_options(dhcp)?;

    let ovn = Arc::new(Ovn::new("10.4.3.1", 6641));
    LogicalSwitch::get_by_name(ovn, name)?.set_cidr(&dhcp.cidr)?;
    Ok(())
}

fn connect_router_to_ls(
    router: &mut LogicalRouter,
    switch: &mut LogicalSwitch,
    address: &str,
) -> Result<(), Error> {
    let lrp_name = format!("lr_{}_ls_{}", router.name(), switch.name());

    router
        .lrp()
        .create_if_missing(&lrp_name, address)?
        // TODO: do something about redundant update if LRP already exists
        .update(address)?;

    let lsp_name = format!("ls_{}_lr_{}", switch.name(), router.name());
    let params = json!({
        "type": "router",
        "addresses": "router",
        "options": ["map", [ ["router-port", lrp_name] ]]
    });
    switch
        .lsp()
        .create_if_missing(&lsp_name, Some(params.as_object().unwrap()))?;
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

    let ovn = Arc::new(Ovn::new("10.4.3.1", 6641));
    let lr_name = format!("{}-{}", &namespace, &name);
    let mut lr = LogicalRouter::get_by_name(ovn.clone(), &lr_name)?;

    let ls_name = name_namespaced(network);
    let mut ls = LogicalSwitch::get_by_name(ovn, &ls_name)?;

    connect_router_to_ls(&mut lr, &mut ls, &router_attachment.address)?;

    Ok(())
}

fn delete_network(name: &str) -> Result<(), Error> {
    let ovn = Arc::new(Ovn::new("10.4.3.1", 6641));
    LogicalSwitch::get_by_name(ovn, name)?.delete()
}

fn delete_router(name: &str) -> Result<(), Error> {
    let ovn = Arc::new(Ovn::new("10.4.3.1", 6641));
    LogicalRouter::get_by_name(ovn, name)?.delete()
}

/// Handle updates to networks in the cluster
async fn reconcile_network(network: Arc<Network>, ctx: Arc<State>) -> Result<Action, Error> {
    let mut network = (*network).clone();
    let ovn = Arc::new(Ovn::new("10.4.3.1", 6641));
    let client = ctx.client.clone();
    let name = name_namespaced(&network);

    if network.metadata.deletion_timestamp.is_some() {
        println!("ovn: Network {} waiting for deletion", name);
        delete_network(&name)?;
        network
            .remove_finalizer("ovn", client, "cluster-controller.ovn")
            .await?;
        println!("ovn: Network {} deleted", name);
    } else {
        println!("ovn: update for network {name}");
        network
            .ensure_finalizer("ovn", client.clone(), "cluster-controller.ovn")
            .await?;
        LogicalSwitch::create_if_missing(ovn, &name)?;
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
    let ovn = Arc::new(Ovn::new("10.4.3.1", 6641));
    let namespace = ResourceExt::namespace(vm).expect("Failed to get VM namespace");
    let network_name = nic.name.as_ref().expect("No network name set");
    let mac_address = nic.mac_address.as_ref().expect("MAC address missing");

    let ls_name = format!("{}-{}", &namespace, &network_name);
    let mut ls = LogicalSwitch::get_by_name(ovn.clone(), &ls_name)?;

    let lsp_id = nic.ovn_id.as_ref().unwrap();
    let mut lsp = ls.lsp().create_if_missing(lsp_id, None)?;

    lsp.set_address(mac_address)?;

    let api: Api<Network> = Api::namespaced(client.clone(), &namespace);
    let network = api.get(network_name).await?;
    if let Some(dhcp) = network.spec.dhcp {
        lsp.set_dhcp_options(&dhcp.cidr)?;
    }
    Ok(())
}

fn disconnect_vm_nic(vm: &VirtualMachine, nic: &NetworkAttachment) -> Result<(), Error> {
    let ovn = Arc::new(Ovn::new("10.4.3.1", 6641));
    let ls_name = format!(
        "{}-{}",
        ResourceExt::namespace(vm).expect("Failed to get VM namespace"),
        nic.name.as_ref().expect("No network name set")
    );
    if LogicalSwitchPort::get_by_name(ovn.clone(), nic.ovn_id.as_ref().unwrap()).is_ok() {
        println!("ovn: lsp exists for NIC, removing");
        let mut ls = LogicalSwitch::get_by_name(ovn, &ls_name)?;
        ls.del_lsp(nic.ovn_id.as_ref().unwrap())?;
    }
    Ok(())
}

fn get_vm_ovn_nics(vm: &VirtualMachine) -> Vec<NetworkAttachment> {
    match &vm.status {
        Some(status) => status
            .networks
            .clone()
            .into_iter()
            .filter(|net| net.ovn_id.is_some())
            .collect(),
        None => Vec::new(),
    }
}

/// Handle updates to VMs in the cluster
async fn reconcile_vm(vm: Arc<VirtualMachine>, ctx: Arc<State>) -> Result<Action, Error> {
    let mut vm = (*vm).clone();
    let name = name_namespaced(&vm);
    let client = ctx.client.clone();

    if vm.metadata.deletion_timestamp.is_some() {
        println!("ovn: VM {name} waiting for deletion");
        for (index, nic) in get_vm_ovn_nics(&vm).iter().enumerate() {
            println!("ovn: disconnecting NIC {index} for VM {name}");
            disconnect_vm_nic(&vm, nic)?;
        }
        vm.remove_finalizer("ovn", client, "cluster-controller.ovn")
            .await?;
        ok_no_requeue!()
    } else {
        println!("ovn: VM {name} updated");
        vm.ensure_finalizer("ovn", client.clone(), "cluster-controller.ovn")
            .await?;
        let mut ip_addresses: Vec<String> = Vec::new();
        for (index, nic) in get_vm_ovn_nics(&vm).iter().enumerate() {
            println!("ovn: connecting NIC {index} for VM {name}");
            connect_vm_nic(client.clone(), &vm, nic).await?;
            if let Some(ovn_id) = nic.ovn_id.as_ref() {
                let ovn = Arc::new(Ovn::new("10.4.3.1", 6641));
                let lsp = LogicalSwitchPort::get_by_name(ovn, ovn_id)?;
                if let Some(ip) = lsp.dynamic_ip() {
                    ip_addresses.push(ip);
                }
            }
        }

        if let Some(status) = vm.status.clone() {
            let ip_addresses_string = ip_addresses.join(",");
            let new_status = VirtualMachineStatus {
                ip_addresses: Some(ip_addresses),
                ip_addresses_string: Some(ip_addresses_string),
                ..status
            };
            set_vm_status(&vm, new_status, client).await?;
        }

        ok_and_requeue!(600)
    }
}

/// Handle updates to routers in the cluster
async fn reconcile_router(router: Arc<Router>, ctx: Arc<State>) -> Result<Action, Error> {
    let mut router = (*router).clone();
    let ovn = Arc::new(Ovn::new("10.4.3.1", 6641));
    let client = ctx.client.clone();
    let namespace = router
        .metadata
        .namespace
        .as_ref()
        .expect("get router namespace")
        .clone();
    let name = name_namespaced(&router);

    if router.metadata.deletion_timestamp.is_some() {
        println!("ovn: Router {} waiting for deletion", name);
        delete_router(&name)?;
        router
            .remove_finalizer("ovn", client, "cluster-controller.ovn")
            .await?;
        println!("ovn: Router {} deleted", name);
    } else {
        println!("ovn: update for router {name}");
        router
            .ensure_finalizer("ovn", client.clone(), "cluster-controller.ovn")
            .await?;

        let mut lr = LogicalRouter::create_if_missing(ovn, &name)?;
        if let Some(routes) = &router.spec.routes {
            ensure_router_routes(&name, routes)?;
        } else {
            ensure_router_routes(&name, &[])?;
        }

        if let Some(true) = &router.spec.metadata_service {
            deploy_mds(
                client.clone(),
                "ovn-cluster-controller",
                &namespace,
                router.metadata.name.as_ref().expect("get router name"),
            )
            .await?;

            connect_metadataservice(&mut lr).await?;
        }

        println!("ovn: update for router {name} successful");

        let status = RouterStatus { is_created: true };
        set_router_status(&router, status, client.clone()).await?;
    }

    ok_and_requeue!(600)
}

async fn connect_metadataservice(lr: &mut LogicalRouter) -> Result<(), Error> {
    let ovn = Arc::new(Ovn::new("10.4.3.1", 6641));
    let mds_name = format!("mds-{}", lr.name());
    let mut ls = LogicalSwitch::create_if_missing(ovn, &mds_name)?;
    connect_router_to_ls(lr, &mut ls, "169.254.169.253/30")?;

    ls.lsp()
        .create_if_missing(&mds_name, None)?
        .set_address("02:00:00:00:00:02")?;
    Ok(())
}

fn error_policy<T>(_object: Arc<T>, _error: &Error, _ctx: Arc<State>) -> Action {
    Action::requeue(Duration::from_secs(15))
}

pub async fn create(client: Client) -> Result<(), Error> {
    let context = Arc::new(State {
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
