use crate::cluster::libvirt::scheduling;
use crate::cluster::libvirt::scheduling::{
    clear_successful_migration, is_uncompliant, migration_requested,
};
use crate::cluster::libvirt::utils::{fill_nics, fill_uuid};
use crate::crd::libvirt::{set_vm_status, VirtualMachine, VirtualMachineStatus};
use crate::errors::Error;
use crate::ok_and_requeue;
use crate::utils::extend_traits::{ExtendResource, TryStatus};
use crate::utils::resource_controller::{DefaultState, ResourceControllerBuilder};
use crate::utils::strings::field_manager;
use kube::runtime::controller::Action;
use kube::Client;
use lazy_static::lazy_static;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::Duration;
use tracing::{info, info_span, Instrument};

lazy_static! {
    static ref FIELD_MANAGER: String = field_manager("libvirt.vm");
    static ref SCHEDULE_MUTEX: Mutex<()> = Mutex::new(());
}

async fn delete_fn(_vm: Arc<VirtualMachine>, _ctx: Arc<DefaultState>) -> Result<Action, Error> {
    Ok(Action::await_change())
}

async fn create_fn(vm: Arc<VirtualMachine>, ctx: Arc<DefaultState>) -> Result<Action, Error> {
    let client = ctx.client.clone();
    let mut vm = vm.as_ref().to_owned();
    let name = vm.name_prefixed_with_namespace();
    info!("libvirt: beginning to reconcile: {}", name);

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

    // Check if we have a pending migration request
    let migration_required = migration_requested(&vm);

    // Check if we are non-compliant with anti-affinity groups
    let reschedule_required = is_uncompliant(&vm, client.clone()).await?;

    if !status.scheduled || migration_required || reschedule_required {
        let _mutex = SCHEDULE_MUTEX
            .lock()
            .instrument(info_span!("wait for scheduler mutex"))
            .await;
        info!("libvirt: Acquired mutex to schedule: {}", name);

        // Schedule normally
        let schedule_result = scheduling::schedule(&vm, false, client.clone()).await;
        // If scheduling failed and we have requested a migration, allow bypassing of affinity
        // so that we can temporarily remove a hypervisor when N(affinity group) == N(hypervisors)
        let node = if migration_required && schedule_result.is_err() {
            scheduling::schedule(&vm, true, client.clone()).await?
        } else {
            schedule_result?
        };
        status.node = Some(node.metadata.name.expect("Unknown node name"));
        status.scheduled = true;

        if migration_required {
            status.migration_pending = true;
        }

        // Status must be updated before we release the scheduling mutex
        set_vm_status(&vm, status, client.clone()).await?;
    }

    clear_successful_migration(&mut vm, client.clone(), &FIELD_MANAGER).await?;

    info!("libvirt: updated: {}", name);
    ok_and_requeue!(600)
}

pub async fn create(client: Client) -> Result<(), Error> {
    info!("libvirt: Starting vm controller");
    ResourceControllerBuilder::new(client)
        .with_default_state()
        .with_default_error_policy()
        .with_functions(create_fn, delete_fn)
        .run()
        .await;
    Ok(())
}
