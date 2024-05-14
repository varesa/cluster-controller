use k8s_openapi::api::core::v1::Node;
use kube::runtime::controller::Action;
use kube::{
    api::{Api, ListParams},
    Client, ResourceExt,
};
use std::sync::Arc;
use tokio::time::Duration;
use tracing::{info, instrument};

use crate::cluster::{MAINTENANCE_ANNOTATION, MIGRATION_REQUEST_ANNOTATION};
use crate::crd::libvirt::VirtualMachine;
use crate::errors::Error;
use crate::ok_and_requeue;
use crate::utils::extend_traits::{ExtendResource, TryStatus};
use crate::utils::resource_controller::{DefaultState, ResourceControllerBuilder};

#[instrument(skip(_ctx))]
async fn delete_fn(_vm: Arc<Node>, _ctx: Arc<DefaultState>) -> Result<Action, Error> {
    Ok(Action::await_change())
}

#[instrument(skip(client))]
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
#[instrument(skip(ctx))]
async fn update_fn(node: Arc<Node>, ctx: Arc<DefaultState>) -> Result<Action, Error> {
    let client = ctx.client.clone();
    let node = node.as_ref().to_owned();
    let name = node.name_unchecked();
    info!("libvirt: beginning to reconcile: {}", name);

    if let Some(annotations) = node.metadata.annotations.as_ref() {
        if let Some(value) = annotations.get(MAINTENANCE_ANNOTATION) {
            if value.to_lowercase() == "true" {
                request_reschedule_node_vms(&node, client).await?;
            }
        }
    }

    info!("libvirt: updated: {}", name);
    ok_and_requeue!(600)
}

#[instrument(skip(client))]
pub async fn create(client: Client) -> Result<(), Error> {
    info!("libvirt: Starting node controller");
    ResourceControllerBuilder::new(client)
        .with_default_state()
        .with_default_error_policy()
        .with_functions(update_fn, delete_fn)
        .run()
        .await;
    Ok(())
}
