use crate::crd::libvirt::{set_vm_status, NetworkAttachment, VirtualMachine, VirtualMachineStatus};
use crate::errors::Error;
use crate::utils::extend_traits::{ExtendResource, TryStatus};
use kube::Client;
use serde_json::json;
use sha2::{Digest, Sha256};
use tracing::instrument;
use uuid::Uuid;

const PREFIX: &str = "52:54:00";

/// Takes a VM name and a network interface specification and generates
/// a MAC address based on a hash of the information
pub fn generate_mac_address(vm_name: &str, nic: &NetworkAttachment, index: usize) -> String {
    let mut hasher = Sha256::new();
    hasher.update(vm_name);
    let network = nic.name.clone();
    let bridge = nic.bridge.clone();
    hasher.update(
        network
            .or(bridge)
            .expect("bridge or network name should be set"),
    );
    hasher.update(vec![index as u8]);
    let hash = hasher.finalize();
    format!("{}:{:x}:{:x}:{:x}", PREFIX, hash[29], hash[30], hash[31])
}

/// Search for network attachment with the same target (network name or bridge)
fn find_matching_network<'a>(
    list: &'a [NetworkAttachment],
    network: &'a NetworkAttachment,
) -> Option<&'a NetworkAttachment> {
    if network.name.is_some() {
        list.iter().find(|candidate| candidate.name == network.name)
    } else if network.bridge.is_some() {
        list.iter()
            .find(|candidate| candidate.bridge == network.bridge)
    } else {
        panic!("A network with neither name nor bridge should not exist")
    }
}

/// For every network in the VM spec, add MAC addresses and OVN port IDs where necessary and not
/// specified. Save the extra information in the status subresource
#[instrument(skip(client))]
pub async fn fill_nics(vm: &mut VirtualMachine, client: Client) -> Result<(), Error> {
    let vm_name = vm.name_prefixed_with_namespace();

    let status_networks = vm.try_status()?.networks.clone();
    let mut new_status_networks = Vec::new();

    for (index, nic_spec) in vm.spec.networks.iter_mut().enumerate() {
        let mut nic_status = find_matching_network(&status_networks, nic_spec)
            .cloned()
            .unwrap_or(NetworkAttachment {
                name: nic_spec.name.clone(),
                bridge: nic_spec.bridge.clone(),
                ..NetworkAttachment::default()
            });

        // Update number of queues
        nic_status.queues = nic_spec.queues;

        // Generate a new MAC address if not set
        if nic_spec.mac_address.is_some() {
            nic_status.mac_address.clone_from(&nic_spec.mac_address);
        } else if nic_status.mac_address.is_none() {
            nic_status.mac_address = Some(generate_mac_address(&vm_name, nic_spec, index));
        }

        // Generate a new OVN port ID if not set and using OVN network
        if nic_spec.name.is_some() {
            if nic_spec.ovn_id.is_some() {
                nic_status.ovn_id.clone_from(&nic_spec.ovn_id);
            } else if nic_status.ovn_id.is_none() {
                nic_status.ovn_id = Some(
                    Uuid::new_v4()
                        .hyphenated()
                        .encode_lower(&mut Uuid::encode_buffer())
                        .into(),
                );
            }
        }
        new_status_networks.push(nic_status);
    }
    if json!(status_networks) != json!(new_status_networks) {
        let new_status = VirtualMachineStatus {
            networks: new_status_networks,
            ..vm.try_status()?.clone()
        };
        set_vm_status(vm, new_status, client.clone()).await?;
    }
    vm.commit(client, "cluster-manager.libvirt").await?;
    Ok(())
}

/// Create a libvirt UUID for a VM if not yet set
#[instrument(skip(client))]
pub async fn fill_uuid(vm: &mut VirtualMachine, client: Client) -> Result<(), Error> {
    if vm.spec.uuid.is_none() {
        vm.spec.uuid = Some(
            Uuid::new_v4()
                .hyphenated()
                .encode_lower(&mut Uuid::encode_buffer())
                .into(),
        );
        vm.commit(client, "cluster-manager.libvirt").await?;
    }
    Ok(())
}
