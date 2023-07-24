use futures::StreamExt;
use k8s_openapi::api::core::v1::Node;
use kube::runtime::controller::{Action, Controller};
use kube::{
    api::{Api, ListParams},
    Client, ResourceExt,
};
use std::sync::Arc;
use tokio::time::Duration;

use crate::cluster::libvirt::controller::{MAINTENANCE_ANNOTATION, MIGRATION_REQUEST_ANNOTATION};
use crate::crd::libvirt::VirtualMachine;
use crate::errors::Error;
use crate::utils::extend_traits::{ExtendResource, TryStatus};
use crate::{create_controller, ok_and_requeue};

/// State available for the reconcile and error_policy functions
/// called by the Controller
struct State {
    client: Client,
}

async fn request_reschedule_node_vms(node: &Node, client: Client) -> Result<(), Error> {
    let vms: Api<VirtualMachine> = Api::all(client.clone());
    if let Ok(mut list) = vms.list(&ListParams::default()).await {
        for vm in list.iter_mut() {
            if let Some(scheduled_node) = &vm.try_status()?.node {
                if scheduled_node == &node.name_unchecked() {
                    vm.annotations_mut().insert(
                        String::from(MIGRATION_REQUEST_ANNOTATION),
                        node.name_unchecked(),
                    );
                    vm.commit(client.clone(), "cluster-manager.libvirt.node")
                        .await?;
                }
            }
        }
    }
    Ok(())
}

/// Handle updates to nodes in the cluster
async fn reconcile(node: Arc<Node>, ctx: Arc<State>) -> Result<Action, Error> {
    let client = ctx.client.clone();
    let node = node.as_ref().to_owned();
    let name = node.name_unchecked();
    println!("libvirt: beginning to reconcile: {}", name);

    if let Some(annotations) = node.metadata.annotations.as_ref() {
        if let Some(value) = annotations.get(MAINTENANCE_ANNOTATION) {
            if value.to_lowercase() == "true" {
                request_reschedule_node_vms(&node, client).await?;
            }
        }
    }

    println!("libvirt: updated: {}", name);
    ok_and_requeue!(600)
}

fn error_policy(_object: Arc<Node>, _error: &Error, _ctx: Arc<State>) -> Action {
    Action::requeue(Duration::from_secs(15))
}

pub async fn create(client: Client) -> Result<(), Error> {
    let context = Arc::new(State {
        client: client.clone(),
    });
    let nodes: Api<Node> = Api::all(client.clone());
    println!("libvirt: Starting node controller");
    create_controller!(nodes, reconcile, error_policy, context);
    Ok(())
}
