use futures::StreamExt;
use kube::runtime::controller::{Action, Controller};
use kube::{
    api::{Api, ListParams},
    Client,
};
use lazy_static::lazy_static;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::Duration;
use uuid::Uuid;

use crate::cluster::libvirt::{scheduling, utils::generate_mac_address};
use crate::crd::libvirt::{set_vm_status, NetworkAttachment, VirtualMachine, VirtualMachineStatus};
use crate::errors::Error;
use crate::utils::{name_namespaced, ExtendResource, TryStatus};
use crate::{create_controller, ok_and_requeue};

/// State available for the reconcile and error_policy functions
/// called by the Controller
struct State {
    client: Client,
}

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

async fn fill_nics(vm: &mut VirtualMachine, client: Client) -> Result<(), Error> {
    let vm_name = name_namespaced(vm);

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

        // Generate a new MAC address if not set
        if nic_spec.mac_address.is_some() {
            nic_status.mac_address = nic_spec.mac_address.clone();
        } else if nic_status.mac_address.is_none() {
            nic_status.mac_address = Some(generate_mac_address(&vm_name, nic_spec, index));
        }

        // Generate a new OVN port ID if not set and using OVN network
        if nic_spec.name.is_some() {
            if nic_spec.ovn_id.is_some() {
                nic_status.ovn_id = nic_spec.ovn_id.clone();
            } else if nic_status.ovn_id.is_none() {
                nic_status.ovn_id = Some(
                    Uuid::new_v4()
                        .to_hyphenated()
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

async fn fill_uuid(vm: &mut VirtualMachine, client: Client) -> Result<(), Error> {
    if vm.spec.uuid.is_none() {
        vm.spec.uuid = Some(
            Uuid::new_v4()
                .to_hyphenated()
                .encode_lower(&mut Uuid::encode_buffer())
                .into(),
        );
        vm.commit(client, "cluster-manager.libvirt").await?;
    }
    Ok(())
}

lazy_static! {
    static ref SCHEDULE_MUTEX: Mutex<()> = Mutex::new(());
}

/// Handle updates to volumes in the cluster
async fn reconcile(vm: Arc<VirtualMachine>, ctx: Arc<State>) -> Result<Action, Error> {
    let client = ctx.client.clone();
    let mut vm = vm.as_ref().to_owned();
    let name = name_namespaced(&vm);
    println!("libvirt: beginning to reconcile: {}", name);

    if vm.metadata.deletion_timestamp.is_some() {
        println!("libvirt: VM {} waiting for deletion", name);
        return ok_and_requeue!(600);
    }

    if vm.status.is_none() {
        set_vm_status(
            &vm,
            VirtualMachineStatus {
                scheduled: false,
                running: false,
                migration_pending: false,
                node: None,
                domain_name: name.clone(),
                ip_addresses: None,
                ip_addresses_string: None,
                networks: vec![],
            },
            client.clone(),
        )
        .await?;
    }

    fill_nics(&mut vm, client.clone()).await?;
    fill_uuid(&mut vm, client.clone()).await?;

    let mut status = vm.try_status()?.clone();

    if !status.scheduled {
        let _mutex = SCHEDULE_MUTEX.lock().await;
        println!("libvirt: Acquired mutex to schedule: {}", name);

        let node = scheduling::schedule(&vm, client.clone()).await?;
        status.node = Some(node.metadata.name.expect("Unknown node name"));
        status.scheduled = true;

        // Status must be updated before we release the scheduling mutex
        set_vm_status(&vm, status, client.clone()).await?;
    }

    println!("libvirt: updated: {}", name);
    ok_and_requeue!(600)
}

fn error_policy(_error: &Error, _ctx: Arc<State>) -> Action {
    Action::requeue(Duration::from_secs(15))
}

pub async fn create(client: Client) -> Result<(), Error> {
    let context = Arc::new(State {
        client: client.clone(),
    });
    let vms: Api<VirtualMachine> = Api::all(client.clone());
    println!("libvirt: Starting vm controller");
    create_controller!(vms, reconcile, error_policy, context);
    Ok(())
}
