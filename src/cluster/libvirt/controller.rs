use futures::StreamExt;
use k8s_openapi::api::core::v1::Node;
use kube::runtime::controller::{Context, Controller, ReconcilerAction};
use kube::{
    api::{Api, ListParams, PostParams, ResourceExt},
    Client,
};
use serde_json::json;
use tokio::time::Duration;

use crate::cluster::libvirt::utils::generate_mac_address;
use crate::crd::libvirt::{VirtualMachine, VirtualMachineStatus};
use crate::errors::Error;
use crate::utils::name_namespaced;
use crate::{
    api_replace_resource, client_replace_resource, create_controller, create_set_status,
    ok_and_requeue,
};
use uuid::Uuid;

/// State available for the reconcile and error_policy functions
/// called by the Controller
struct State {
    client: Client,
}

create_set_status!(VirtualMachine, VirtualMachineStatus);

async fn schedule(_vm: &VirtualMachine, client: Client) -> Result<Node, Error> {
    let node_api: Api<Node> = Api::all(client.clone());
    let nodes = node_api.list(&ListParams::default()).await?;
    /*for node in nodes {
        println!("Candidate: {}", ResourceExt::name(&node));
    }*/
    Ok(nodes.items[0].clone())
    //Err(Error::Timeout(String::from("asd")))
}

async fn fill_nics(vm: &mut VirtualMachine, client: Client) -> Result<(), Error> {
    let vm_name = name_namespaced(vm);
    for (index, nic) in vm.spec.networks.iter_mut().enumerate() {
        // Generate a new MAC address if not set
        if nic.mac_address.is_none() {
            let new_mac = generate_mac_address(&vm_name, nic, index);
            nic.mac_address = Some(new_mac.clone());
        }

        // Generate a new OVN port ID if not set and using OVN network
        if nic.name.is_some() && nic.ovn_id.is_none() {
            nic.ovn_id = Some(
                Uuid::new_v4()
                    .to_hyphenated()
                    .encode_lower(&mut Uuid::encode_buffer())
                    .into(),
            );
        }
    }
    client_replace_resource!(client, VirtualMachine, vm);
    Ok(())
}

async fn fill_uuid(vm: &mut VirtualMachine, client: Client) -> Result<(), Error> {
    if vm.spec.uuid.is_none() {
        vm.spec.uuid = Some(
            Uuid::new_v4()
                .to_hyphenated()
                .encode_lower(&mut Uuid::encode_buffer())
                .into(),
        );
        client_replace_resource!(client, VirtualMachine, vm);
    }
    Ok(())
}

/// Handle updates to volumes in the cluster
async fn reconcile(mut vm: VirtualMachine, ctx: Context<State>) -> Result<ReconcilerAction, Error> {
    let client = ctx.get_ref().client.clone();
    let name = name_namespaced(&vm);

    if vm.metadata.deletion_timestamp.is_some() {
        println!("libvirt: VM {} waiting for deletion", name);
        return ok_and_requeue!(600);
    }

    fill_nics(&mut vm, client.clone()).await?;
    fill_uuid(&mut vm, client.clone()).await?;

    let old_status = vm
        .status
        .clone()
        .or_else(|| {
            Some(VirtualMachineStatus {
                scheduled: false,
                running: false,
                node: None,
                domain_name: String::new(),
            })
        })
        .unwrap();

    let mut new_status = VirtualMachineStatus { ..old_status };

    if new_status.domain_name.is_empty() {
        new_status.domain_name = name.clone();
    }

    if !old_status.scheduled {
        let node = schedule(&vm, client.clone()).await?;
        new_status.node = Some(node.metadata.name.expect("Unknown node name"));
        new_status.scheduled = true;
    }

    set_status(&vm, new_status, client.clone()).await?;

    println!("libvirt: updated: {}", name);
    return ok_and_requeue!(600);
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
    let vms: Api<VirtualMachine> = Api::all(client.clone());
    println!("libvirt: Starting controller");
    create_controller!(vms, reconcile, error_policy, context);
    Ok(())
}
