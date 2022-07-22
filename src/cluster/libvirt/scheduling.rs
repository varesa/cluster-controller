use k8s_openapi::api::core::v1::Node;
use kube::{
    api::{Api, ListParams, ResourceExt},
    Client,
};
use rand::seq::SliceRandom;

use crate::crd::libvirt::v1beta2::VirtualMachine;
use crate::errors::Error;

/// Find all VMs registered to the k8s apiserver with the given label set to the given value
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

/// Try to return the node the VM is scheduled to run on.
/// If the VM has not yet been scheduled, return None
fn get_vm_node(vm: &VirtualMachine) -> Option<String> {
    vm.status.as_ref().and_then(|status| status.node.clone())
}

/// Return all nodes that have a VM scheduled with the given label
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
        candidates.retain(|candidate| candidate.metadata.name.as_ref().unwrap() != node_to_remove);
    }
}

const ANTI_AFFINITY_LABEL: &str = "antiAffinity";

pub(crate) async fn schedule(vm: &VirtualMachine, client: Client) -> Result<Node, Error> {
    let node_api: Api<Node> = Api::all(client.clone());
    let mut candidates = node_api.list(&ListParams::default()).await?;

    let labels = ResourceExt::labels(vm);
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

    if let Some(node) = candidates.items.choose(&mut rand::thread_rng()) {
        Ok(node.clone())
    } else {
        Err(Error::ScheduleFailed(vm.metadata.name.clone().unwrap()))
    }
}
