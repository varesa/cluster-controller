use k8s_openapi::api::core::v1::Node;
use kube::runtime::controller::Action;
use kube::{
    Client, ResourceExt,
    api::{Api, ListParams},
};
use std::sync::Arc;
use tokio::time::Duration;
use tracing::{info, instrument};

use crate::crd::libvirt::VirtualMachine;
use crate::errors::Error;
use crate::ok_and_requeue;
use crate::utils::resource_controller::{DefaultState, ResourceControllerBuilder};
use crate::utils::traits::kube::TryStatus;
use crate::utils::traits::node::NodeExt;
use crate::utils::traits::virtualmachine::VirtualMachineExt;

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
                    vm.request_migration_away_from(
                        node,
                        "cluster-manager.libvirt.node",
                        client.clone(),
                    )
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

    if node.in_maintenance_mode() {
        request_reschedule_node_vms(&node, client).await?;
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
