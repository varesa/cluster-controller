use kube::{Client, api::{Api, ListParams, ResourceExt, PostParams}};
use kube::runtime::controller::{Context, Controller, ReconcilerAction};
use k8s_openapi::api::core::v1::Node;
use tokio::time::Duration;
use futures::StreamExt;
//use humanize_rs::bytes::Bytes;
use serde_json::json;

//use crate::GROUP_NAME;
use crate::errors::Error;
use crate::crd::libvirt::{VirtualMachine,VirtualMachineStatus};
use crate::cluster::libvirt::utils::generate_mac_address;
use crate::utils::name_namespaced;
use crate::create_controller;
use uuid::Uuid;

/// State available for the reconcile and error_policy functions
/// called by the Controller
struct State {
    client: Client,
}

async fn set_status(vm: &VirtualMachine, status: VirtualMachineStatus, client: Client) -> Result<(), Error> {
    let vms: Api<VirtualMachine> = Api::namespaced(client.clone(), &ResourceExt::namespace(vm).expect("get VM namespace"));
    let status_update = json!({
        "apiVersion": vm.api_version,
        "kind": vm.kind,
        "metadata": {
            "name": ResourceExt::name(vm),
            "resourceVersion": ResourceExt::resource_version(vm),
        },
        "status": status,
    });
    vms.replace_status(&ResourceExt::name(vm), &PostParams::default(), serde_json::to_vec(&status_update).expect("serialize status")).await?;
    Ok(())
}

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
            nic.ovn_id = Some(Uuid::new_v4().to_hyphenated().encode_lower(&mut Uuid::encode_buffer()).into());
        }
    }
    let vms: Api<VirtualMachine> = Api::namespaced(client, &ResourceExt::namespace(vm).expect("get VM namespace"));
    vms.replace(&ResourceExt::name(vm), &PostParams::default(), vm).await?;
    Ok(())
}

async fn fill_uuid(vm: &mut VirtualMachine, client: Client) -> Result<(), Error> {
    if vm.spec.uuid.is_none() {
        vm.spec.uuid = Some(Uuid::new_v4().to_hyphenated().encode_lower(&mut Uuid::encode_buffer()).into());
        let vms: Api<VirtualMachine> = Api::namespaced(client, &ResourceExt::namespace(vm).expect("get VM namespace"));
        vms.replace(&ResourceExt::name(vm), &PostParams::default(), vm).await?;
    }
    Ok(())
}

/// Handle updates to volumes in the cluster
async fn reconcile(mut vm: VirtualMachine, ctx: Context<State>) -> Result<ReconcilerAction, Error> {
    let client = ctx.get_ref().client.clone();
    let name = name_namespaced(&vm);

    fill_nics(&mut vm, client.clone()).await?;
    fill_uuid(&mut vm, client.clone()).await?;
    let node = schedule(&vm, client.clone()).await?;

    let status = VirtualMachineStatus {
        scheduled: false,
        running: false,
        node: Some(node.metadata.name.expect("Unknown node name")),
        domain_name: name.clone(),
    };
    set_status(&vm, status, client.clone()).await?;

    println!("libvirt: updated: {}", name);

    Ok(ReconcilerAction {
        requeue_after: Some(Duration::from_secs(600)),
    })
}

fn error_policy(_error: &Error, _ctx: Context<State>) -> ReconcilerAction {
    ReconcilerAction {
        requeue_after: Some(Duration::from_secs(15)),
    }
}

pub async fn create(client: Client) -> Result<(), Error> {
    let context = Context::new(State { client: client.clone() });
    let vms: Api<VirtualMachine> = Api::all(client.clone());
    println!("libvirt: Starting controller");
    create_controller!(vms, reconcile, error_policy, context);
    Ok(())
}
