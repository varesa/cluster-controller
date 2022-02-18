use futures::StreamExt;
use k8s_openapi::api::core::v1::Node;
use kube::runtime::controller::{Context, Controller, ReconcilerAction};
use kube::{
    api::{Api, ListParams, PostParams, ResourceExt},
    Client,
};
use tokio::time::Duration;
//use humanize_rs::bytes::Bytes;
use serde_json::json;

//use crate::GROUP_NAME;
use crate::crd::ovn::Network;
use crate::create_controller;
use crate::errors::Error;
use crate::utils::name_namespaced;
use uuid::Uuid;

/// State available for the reconcile and error_policy functions
/// called by the Controller
struct State {
    client: Client,
}

/// Handle updates to volumes in the cluster
async fn reconcile(mut network: Network, ctx: Context<State>) -> Result<ReconcilerAction, Error> {
    let client = ctx.get_ref().client.clone();
    let name = name_namespaced(&network);

    println!("ovn: updated: {}", name);

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
    let context = Context::new(State {
        client: client.clone(),
    });
    println!("ovn: Starting controller");

    let vms: Api<Network> = Api::all(client.clone());
    create_controller!(vms, reconcile, error_policy, context);
    Ok(())
}
