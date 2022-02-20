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
use crate::crd::ovn::{Network, NetworkStatus};
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

create_set_status!(Network, NetworkStatus);

fn ensure_exists(name: &str) {
    let mut ovn = Ovn::new("10.4.3.1", 6641);
    if ovn.get_ls(name).is_none() {
        println!("ovn: Sw {name} doesn't exist, creating");
        ovn.add_ls(name);
    }
}

fn delete(name: &str) -> Result<(), Error> {
    let mut ovn = Ovn::new("10.4.3.1", 6641);
    ovn.del_ls_by_name(name)?;
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
        delete(&name)?;
        client_remove_finalizer!(client, Network, &network, "ovn");
        println!("ovn: Network {} deleted", name);
    } else {
        println!("ovn: update for {name}");
        client_ensure_finalizer!(client, Network, &network, "ovn");
        ensure_exists(&name);
        println!("ovn: update for {name} successful");

        let status = NetworkStatus { is_created: true };
        set_status(&network, status, client.clone()).await?;
    }

    ok_and_requeue!(600)
}

fn connect_vm_nic(vm: &VirtualMachine, nic: &NetworkAttachment) -> Result<(), Error> {
    let mut ovn = Ovn::new("10.4.3.1", 6641);
    let ls_name = format!(
        "{}-{}",
        ResourceExt::namespace(vm).expect("Failed to get VM namespace"),
        nic.name.as_ref().expect("No network name set")
    );
    if ovn.get_lsp(nic.ovn_id.as_ref().unwrap()).is_none() {
        println!("ovn: lsp missing for NIC, creating");
        ovn.add_lsp(&ls_name, nic.ovn_id.as_ref().unwrap())?;
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
    if ovn.get_lsp(nic.ovn_id.as_ref().unwrap()).is_some() {
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

        ok_no_requeue!()
    } else {
        println!("ovn: VM {name} updated");
        client_ensure_finalizer!(client, VirtualMachine, &vm, "ovn");
        for (index, nic) in get_vm_ovn_nics(&vm).iter().enumerate() {
            println!("ovn: connecting NIC {index} for VM {name}");
            connect_vm_nic(&vm, nic)?;
        }

        ok_and_requeue!(600)
    }
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
    let vms: Api<VirtualMachine> = Api::all(client.clone());

    let context_clone = context.clone();
    let vm_task = tokio::task::spawn(async {
        panic!(
            "OVN VM-controller task exited: {:?}",
            create_controller!(vms, reconcile_vm, error_policy, context_clone)
        );
    });

    let network_task = tokio::task::spawn(async {
        panic!(
            "OVN Network-controller task exited: {:?}",
            create_controller!(networks, reconcile_network, error_policy, context)
        );
    });

    let _ = tokio::try_join!(vm_task, network_task);
    Ok(())
}
