use crate::cluster::{MAINTENANCE_ANNOTATION, MIGRATION_REQUEST_ANNOTATION};
use k8s_openapi::api::core::v1::Node;
use kube::{
    api::{Api, ListParams, ResourceExt},
    Client,
};
use rand::seq::SliceRandom;
use tracing::instrument;

use crate::crd::libvirt::VirtualMachine;
use crate::errors::Error;
use crate::utils::extend_traits::{ExtendResource, TryStatus};

/// Find all VMs registered to the k8s apiserver with the given label set to the given value
#[instrument(skip(client))]
async fn get_vms_with_label(
    client: Client,
    label: &str,
    value: &str,
) -> Result<Vec<VirtualMachine>, Error> {
    let vm_api: Api<VirtualMachine> = Api::all(client.clone());
    let list_params = ListParams {
        label_selector: Some(format!("{label}={value}")),
        ..ListParams::default()
    };
    let conflicting_vms = vm_api.list(&list_params).await?;
    Ok(conflicting_vms.items)
}

/// Return all VMs except self in the same anti-affinity group as the given VM
#[instrument(skip(client))]
async fn get_others_in_affinity_group(
    client: Client,
    ref_vm: &VirtualMachine,
) -> Result<Vec<VirtualMachine>, Error> {
    let maybe_group = ref_vm.labels().get(ANTI_AFFINITY_LABEL);
    if let Some(group) = maybe_group {
        let mut vms = get_vms_with_label(client, ANTI_AFFINITY_LABEL, group).await?;
        vms.retain(|vm| vm.name_prefixed_with_namespace() != ref_vm.name_prefixed_with_namespace());
        Ok(vms)
    } else {
        Ok(Vec::new())
    }
}

/// Check for compliance with anti-affinity groups
#[instrument(skip(client))]
pub async fn is_uncompliant(vm: &VirtualMachine, client: Client) -> Result<bool, Error> {
    let status = vm.try_status()?.clone();

    if status.scheduled {
        let anti_affinity_members = get_others_in_affinity_group(client.clone(), vm).await?;
        let uncompliant = anti_affinity_members.iter().any(|other_vm| {
            let mynode = vm.status.as_ref().and_then(|status| status.node.as_ref());
            let othernode = other_vm
                .status
                .as_ref()
                .and_then(|status| status.node.as_ref());

            mynode.is_some() && mynode == othernode
        });

        return Ok(uncompliant);
    }

    Ok(false)
}

/// Check if the VM has the annotation signaling a migration request set, and if it's value
/// corresponds to the current node. If the annotation is present, but points to another node
/// (e.g. post-migration state), ignore it.
pub fn migration_requested(vm: &VirtualMachine) -> bool {
    let current_node = get_vm_node(vm);

    if let Some(node_to_leave) = vm.annotations().get(MIGRATION_REQUEST_ANNOTATION) {
        current_node == Some(node_to_leave.clone())
    } else {
        false
    }
}

/// Checks if the VM has the annotation signaling a migration request set, and if it points to some
/// other node as a result of a completed migration. Clear the label in that case.
#[instrument(skip(client))]
pub async fn clear_successful_migration(
    vm: &mut VirtualMachine,
    client: Client,
    field_manager: &str,
) -> Result<(), Error> {
    let current_node = get_vm_node(vm);
    if let Some(node_to_leave) = vm.annotations().get(MIGRATION_REQUEST_ANNOTATION) {
        if current_node != Some(node_to_leave.clone()) {
            vm.annotations_mut().remove(MIGRATION_REQUEST_ANNOTATION);
            vm.commit(client.clone(), field_manager).await?;
        }
    }
    Ok(())
}

/// Try to return the node the VM is scheduled to run on.
/// If the VM has not yet been scheduled, return None
fn get_vm_node(vm: &VirtualMachine) -> Option<String> {
    vm.status.as_ref().and_then(|status| status.node.clone())
}

/// Return all nodes that have a VM scheduled with the given label
#[instrument(skip(client))]
async fn get_nodes_with_label_scheduled(
    client: Client,
    label: &str,
    value: &str,
) -> Result<Vec<String>, Error> {
    let vms_scheduled = get_vms_with_label(client.clone(), label, value).await?;
    let nodes_scheduled: Vec<String> = vms_scheduled.iter().filter_map(get_vm_node).collect();
    Ok(nodes_scheduled)
}

/// Remove nodes by the given names from the candidate list
fn remove_candidate_nodes(candidates: &mut Vec<Node>, nodes_to_remove: &Vec<String>) {
    for node_to_remove in nodes_to_remove {
        candidates.retain(|candidate| &candidate.name_unchecked() != node_to_remove);
    }
}

/// Remove all nodes which have the maintenance annotation set
fn remove_nodes_in_maintenance(candidates: &mut Vec<Node>) {
    candidates.retain(|candidate| {
        candidate.annotations().get(MAINTENANCE_ANNOTATION) != Some(&String::from("true"))
    });
}

const ANTI_AFFINITY_LABEL: &str = "antiAffinity";

/// Try to schedule the VM to some node according to rules. Returns either a node-object, or an
/// Error if no node meets the requirements.
///
/// ignore_affinity allows temporarily bypassing anti-affinity rules which can be useful in case
/// of e.g. host maintenance
#[instrument(skip(client))]
pub(crate) async fn schedule(
    vm: &VirtualMachine,
    ignore_affinity: bool,
    client: Client,
) -> Result<Node, Error> {
    let node_api: Api<Node> = Api::all(client.clone());

    // Get all nodes
    let mut candidates = node_api.list(&ListParams::default()).await?;

    // Remove nodes in maintenance
    remove_nodes_in_maintenance(&mut candidates.items);

    // Remove a node we are migrating away from (most of then same as a node in maintenance)
    if let Some(source_node) = vm.annotations().get(MIGRATION_REQUEST_ANNOTATION) {
        remove_candidate_nodes(&mut candidates.items, &vec![source_node.clone()])
    }

    if !ignore_affinity {
        // Remove nodes that already have VMs in the same anti-affinity group
        let labels = vm.labels();
        let anti_affinity_group = labels.get(ANTI_AFFINITY_LABEL);
        if let Some(anti_affinity_group) = anti_affinity_group {
            let blocked_nodes = get_nodes_with_label_scheduled(
                client.clone(),
                ANTI_AFFINITY_LABEL,
                anti_affinity_group,
            )
            .await?;
            remove_candidate_nodes(&mut candidates.items, &blocked_nodes);
        }
    }

    if let Some(node) = candidates.items.choose(&mut rand::thread_rng()) {
        Ok(node.clone())
    } else {
        Err(Error::ScheduleFailed(vm.metadata.name.clone().unwrap()))
    }
}
