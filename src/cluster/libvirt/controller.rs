use kube::{Client, api::{Api, ListParams, Meta, PostParams}};
use kube_runtime::controller::{Context, Controller, ReconcilerAction};
use tokio::time::Duration;
use futures::StreamExt;
use humanize_rs::bytes::Bytes;

use crate::GROUP_NAME;
use crate::errors::Error;
use super::crd::VirtualMachine;

/// State available for the reconcile and error_policy functions
/// called by the Controller
struct State {
    client: Client,
}

/// Handle updates to volumes in the cluster
async fn reconcile(virtualmachine: VirtualMachine, ctx: Context<State>) -> Result<ReconcilerAction, Error> {
    let name = format!(
        "{}-{}",
        Meta::namespace(&virtualmachine).expect("get namespace"),
        Meta::name(&virtualmachine)
    );

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