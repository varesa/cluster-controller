use futures::StreamExt;
use kube::runtime::controller::{Action, Controller};
use kube::{
    api::{Api, ListParams},
    Client,
};
use std::env;
use std::sync::Arc;
use tokio::time::Duration;

use super::lowlevel::Libvirt;
use crate::crd::libvirt::VirtualMachine;
use crate::errors::Error;
use crate::host::libvirt::handlers::LIBVIRT_URI;
use crate::host::libvirt::utils::get_domain_name;
use crate::host::libvirt::{handlers, secrets};
use crate::{create_controller, ok_no_requeue};

/// State available for the reconcile and error_policy functions
/// called by the Controller
pub struct State {
    pub kube: Client,
    pub libvirt: Libvirt,
}

enum Event {
    MissingDomainName,
    Unscheduled,
    NoNode,
    InboundMigration,
    OutboundMigration,
    NotOurs,
    Added,
    Updated,
    Deleted,
}

fn get_event_type(vm: &VirtualMachine, ctx: &Arc<State>) -> Result<Event, Error> {
    let my_node_name = env::var("NODE_NAME").expect("failed to read $NODE_NAME");
    let k8s_vm_name = &vm.metadata.name.clone().expect("VM has no name");
    let is_deleted = &vm.metadata.deletion_timestamp.is_some();

    println!("Received update to {} on {}", k8s_vm_name, my_node_name);

    let libvirt_domain_name = if let Some(name) = get_domain_name(vm) {
        name
    } else {
        return Ok(Event::MissingDomainName);
    };

    let vm_status = vm.status.clone().expect("VM has no status");

    if !vm_status.scheduled {
        return Ok(Event::Unscheduled);
    }

    let target_node = if let Some(name) = vm.status.as_ref().and_then(|status| status.node.as_ref())
    {
        name
    } else {
        return Ok(Event::NoNode);
    };

    let vm_runs_on_us = ctx.libvirt.has_domain(&libvirt_domain_name)?;
    let target_node_is_us = target_node == &my_node_name;
    let migration_pending = vm
        .status
        .as_ref()
        .expect("VM has no status")
        .migration_pending;

    if target_node_is_us && migration_pending {
        return Ok(Event::InboundMigration);
    }

    if !target_node_is_us {
        if vm_runs_on_us {
            return Ok(Event::OutboundMigration);
        } else {
            return Ok(Event::NotOurs);
        }
    }

    match (vm_runs_on_us, is_deleted) {
        (false, false) => Ok(Event::Added),
        (true, false) => Ok(Event::Updated),
        (true, true) => Ok(Event::Deleted),
        (false, true) => Ok(Event::Deleted), // might not exist due to e.g. invalid spec
    }
}

/// Handle updates to volumes in the cluster
async fn reconcile(vm: Arc<VirtualMachine>, ctx: Arc<State>) -> Result<Action, Error> {
    match get_event_type(&vm, &ctx)? {
        Event::Deleted => handlers::handle_delete(&vm, ctx).await,
        Event::Added => handlers::handle_add(&vm, ctx).await,
        Event::OutboundMigration => handlers::handle_outbound_migration(&vm, ctx).await,
        Event::InboundMigration => handlers::handle_inbound_migration(&vm, ctx).await,
        _ => {
            ok_no_requeue!()
        }
    }
}

fn error_policy(_error: &Error, _ctx: Arc<State>) -> Action {
    Action::requeue(Duration::from_secs(15))
}

pub async fn create(client: Client) -> Result<(), Error> {
    let libvirt = Libvirt::new(LIBVIRT_URI)?;
    secrets::ensure_ceph_secret(client.clone(), &libvirt).await?;
    let context = Arc::new(State {
        kube: client.clone(),
        libvirt,
    });
    let vms: Api<VirtualMachine> = Api::all(client.clone());
    println!("Starting libvirt host controller");
    create_controller!(vms, reconcile, error_policy, context);
    Ok(())
}
