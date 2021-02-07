use kube::{Client, api::{Api, ListParams, Meta, PostParams}};
use kube_runtime::controller::{Context, Controller, ReconcilerAction};
use k8s_openapi::api::core::v1::Node;
use tokio::time::Duration;
use futures::StreamExt;
//use humanize_rs::bytes::Bytes;
use serde_json::json;
use virt::{
    connect::Connect,
    domain::Domain,
};

//use crate::GROUP_NAME;
use crate::errors::Error;
use crate::crd::libvirt::{VirtualMachine,VirtualMachineStatus};
use super::lowlevel::Libvirt;

const LIBVIRT_URI: &str = "qemu:///system";


/// State available for the reconcile and error_policy functions
/// called by the Controller
struct State {
    kube: Client,
    libvirt: Libvirt,
}

fn get_domain_name(vm: &VirtualMachine) -> Option<String> {
    let domain_name: Option<&str> = vm.status.as_ref().and_then(|status| Some(status.domain_name.as_ref()));
    match domain_name {
        Some(name) => Some(String::from(name)),
        _ => {
            let namespace = Meta::namespace(vm)
                .or(Some(String::from("<no namespace>")))
                .unwrap();
            println!("Ignored VM {}/{} with no domain name defined",
                     namespace, Meta::name(vm));
            None
        }
    }

}

/// Handle updates to volumes in the cluster
async fn reconcile(vm: VirtualMachine, ctx: Context<State>) -> Result<ReconcilerAction, Error> {
    let my_node_name = "kvm01.p4.esav.fi";

    let vm_name = match get_domain_name(&vm) {
        Some(name) => name,
        None => {
            return Ok(ReconcilerAction { requeue_after: None });
        }
    };

    let target_node = vm.status.as_ref().and_then(|status| status.node.as_ref());
    if let Some(target_node_name) = target_node {
        if target_node_name != my_node_name {
            println!("Ignored VM {} for another host", vm_name);
            return Ok(ReconcilerAction { requeue_after: None });
        }
    } else {
        println!("Ignored unscheduled VM {}", vm_name);
        return Ok(ReconcilerAction { requeue_after: None });
    }

    println!("Update for VM {} that has been scheduled to us", vm_name);
    let libvirt_domain = Domain::lookup_by_name(&ctx.get_ref().libvirt.connection, &vm_name);
    println!("Domain: {:?}", libvirt_domain);

    /*let client = ctx.get_ref().client.clone();

    let status = VirtualMachineStatus {
        scheduled: false,
        running: false,
        node: None,
    };
    set_status(&vm, status, client.clone()).await?;

    println!("Updated: {}", name);*/

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
    let context = Context::new(
        State { kube: client.clone(), libvirt: Libvirt::new(LIBVIRT_URI)? });
    let volumes: Api<VirtualMachine> = Api::all(client.clone());
    println!("Starting libvirt host controller");
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