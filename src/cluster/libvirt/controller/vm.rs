use crate::cluster::libvirt::scheduling;
use crate::cluster::libvirt::utils::{fill_nics, fill_uuid};
use crate::crd::libvirt::{set_vm_status, VirtualMachine, VirtualMachineStatus};
use crate::errors::Error;
use crate::utils::{name_namespaced, TryStatus};
use crate::{create_controller, ok_and_requeue};
use futures::StreamExt;
use kube::runtime::controller::{Action, Controller};
use kube::{api::Api, Client};
use lazy_static::lazy_static;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::Duration;

/// State available for the reconcile and error_policy functions
/// called by the Controller
struct State {
    client: Client,
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

fn error_policy(_object: Arc<VirtualMachine>, _error: &Error, _ctx: Arc<State>) -> Action {
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
