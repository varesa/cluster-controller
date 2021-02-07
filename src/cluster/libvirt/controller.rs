use kube::{Client, api::{Api, ListParams, Meta, PostParams}};
use kube_runtime::controller::{Context, Controller, ReconcilerAction};
use k8s_openapi::api::core::v1::Node;
use tokio::time::Duration;
use futures::StreamExt;
//use humanize_rs::bytes::Bytes;
use serde_json::json;

//use crate::GROUP_NAME;
use crate::errors::Error;
use super::crd::VirtualMachine;
use crate::cluster::libvirt::crd::VirtualMachineStatus;

/// State available for the reconcile and error_policy functions
/// called by the Controller
struct State {
    client: Client,
}

async fn set_status(vm: &VirtualMachine, status: VirtualMachineStatus, client: Client) -> Result<(), Error> {
    let vms: Api<VirtualMachine> = Api::namespaced(client.clone(), &Meta::namespace(vm).expect("get VM namespace"));
    let status_update = json!({
        "apiVersion": vm.api_version,
        "kind": vm.kind,
        "metadata": {
            "name": Meta::name(vm),
            "resourceVersion": Meta::resource_ver(vm),
        },
        "status": status,
    });
    vms.replace_status(&Meta::name(vm), &PostParams::default(), serde_json::to_vec(&status_update).expect("serialize status")).await?;
    Ok(())
}

async fn schedule(vm: &VirtualMachine, client: Client) -> Result<Node, Error> {
    let node_api: Api<Node> = Api::all(client.clone());
    let nodes = node_api.list(&ListParams::default()).await?;
    /*for node in nodes {
        println!("Candidate: {}", Meta::name(&node));
    }*/
    Ok(nodes.items[0].clone())
    //Err(Error::Timeout(String::from("asd")))
}

/// Handle updates to volumes in the cluster
async fn reconcile(vm: VirtualMachine, ctx: Context<State>) -> Result<ReconcilerAction, Error> {
    let client = ctx.get_ref().client.clone();
    let name = format!(
        "{}-{}",
        Meta::namespace(&vm).expect("get namespace"),
        Meta::name(&vm)
    );

    let node = schedule(&vm, client.clone()).await?;

    let status = VirtualMachineStatus {
        scheduled: false,
        running: false,
        node: None,
    };
    set_status(&vm, status, client.clone()).await?;

    println!("Updated: {}", name);

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
    let volumes: Api<VirtualMachine> = Api::all(client.clone());
    println!("Starting libvirt controller");
    Controller::new(volumes, ListParams::default())
        .run(reconcile, error_policy, context)
        .for_each(|res| async move {
            match res {
                Ok(_o) => { /*println!("reconciled {:?}", o)*/ },
                Err(e) => println!("reconcile failed: {:?}", e),
            }
        })
        .await;
    Ok(())
}