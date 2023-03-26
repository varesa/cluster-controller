use futures::StreamExt;
use kube::runtime::controller::{Action, Controller};
use kube::{
    api::{Api, ListParams, PostParams},
    Client,
};
use lazy_static::lazy_static;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::Duration;

use crate::cluster::libvirt::{scheduling, utils::generate_mac_address};
use crate::crd::libvirt::{
    set_vm_status,
    v1beta2::{VirtualMachine, VirtualMachineStatus},
};
use crate::errors::Error;
use crate::utils::name_namespaced;
use crate::{api_replace_resource, client_replace_resource, create_controller, ok_and_requeue};
use uuid::Uuid;

/// State available for the reconcile and error_policy functions
/// called by the Controller
struct State {
    client: Client,
}

async fn fill_nics(vm: &mut VirtualMachine, client: Client) -> Result<(), Error> {
    let vm_name = name_namespaced(vm);
    for (index, nic) in vm.spec.networks.iter_mut().enumerate() {
        // Generate a new MAC address if not set
        if nic.mac_address.is_none() {
            let new_mac = generate_mac_address(&vm_name, nic, index);
            nic.mac_address = Some(new_mac.clone());
        }

        // Generate a new OVN port ID if not set and using OVN network
        if nic.name.is_some() && nic.ovn_id.is_none() {
            nic.ovn_id = Some(
                Uuid::new_v4()
                    .to_hyphenated()
                    .encode_lower(&mut Uuid::encode_buffer())
                    .into(),
            );
        }
    }
    client_replace_resource!(client, VirtualMachine, vm);
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
        client_replace_resource!(client, VirtualMachine, vm);
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

    fill_nics(&mut vm, client.clone()).await?;
    fill_uuid(&mut vm, client.clone()).await?;

    let old_status = vm
        .status
        .clone()
        .or_else(|| {
            Some(VirtualMachineStatus {
                scheduled: false,
                running: false,
                migration_pending: false,
                node: None,
                domain_name: String::new(),
                ip_addresses: None,
                ip_addresses_string: None,
            })
        })
        .unwrap();

    let mut new_status = VirtualMachineStatus { ..old_status };

    if new_status.domain_name.is_empty() {
        new_status.domain_name = name.clone();
    }

    if !old_status.scheduled {
        let _mutex = SCHEDULE_MUTEX.lock().await;
        println!("libvirt: Acquired mutex to schedule: {}", name);

        let node = scheduling::schedule(&vm, client.clone()).await?;
        new_status.node = Some(node.metadata.name.expect("Unknown node name"));
        new_status.scheduled = true;

        // Status must be updated before we release the scheduling mutex
        set_vm_status(&vm, new_status, client.clone()).await?;
    } else {
        // Just update the status without acquiring the mutex
        set_vm_status(&vm, new_status, client.clone()).await?;
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
    println!("libvirt: Starting controller");
    create_controller!(vms, reconcile, error_policy, context);
    Ok(())
}
