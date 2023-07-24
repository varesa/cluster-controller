use crate::crd::libvirt::{set_vm_status, VirtualMachine, VirtualMachineStatus};
use crate::host::libvirt::controller::State;
use crate::host::libvirt::utils::{get_cluster, get_domain_name};
use crate::utils::extend_traits::{ExtendResource, TryStatus};
use crate::utils::strings::field_manager;
use crate::Error;
use crate::{ok_and_requeue, ok_no_requeue};
use kube::runtime::controller::Action;
use lazy_static::lazy_static;
use std::sync::Arc;
use tokio::time::Duration;
use virt::domain::Domain;

pub const LIBVIRT_URI: &str = "qemu:///system";
const NO_BW_LIMIT: u64 = 0;

lazy_static! {
    static ref FIELD_MANAGER: String = field_manager("libvirt-host");
}

pub async fn handle_delete(vm: &VirtualMachine, ctx: Arc<State>) -> Result<Action, Error> {
    let mut vm = (*vm).clone();
    let vm_name = get_domain_name(&vm).expect("VM has a libvirt domain name");
    println!("VM {} waiting for deletion by host controller", vm_name);

    match Domain::lookup_by_name(&ctx.libvirt.connection, &vm_name) {
        Ok(domain) => {
            println!("Domain {vm_name} exists, destroying");
            domain.destroy()?;
            println!("Domain {vm_name} destroyed");
        }
        Err(_) => {
            println!("Domain {vm_name} doesn't exist, ignoring");
        }
    };
    vm.remove_finalizer("libvirt-host", ctx.kube.clone(), &FIELD_MANAGER)
        .await?;

    ok_no_requeue!()
}

pub async fn handle_add(vm: &VirtualMachine, ctx: Arc<State>) -> Result<Action, Error> {
    let mut vm = (*vm).clone();
    let vm_name = get_domain_name(&vm).expect("VM has a libvirt domain name");
    vm.ensure_finalizer("libvirt-host", ctx.kube.clone(), &FIELD_MANAGER)
        .await?;

    // Get cluster capabilities / definition
    let cluster = get_cluster(&ctx).await?;

    ctx.libvirt.create_domain(&vm, &cluster)?;

    let status = VirtualMachineStatus {
        running: true,
        ..vm.status.clone().expect("VM didn't have existing status")
    };
    set_vm_status(&vm, status, ctx.kube.clone()).await?;

    println!("Updated: {}", vm_name);
    ok_and_requeue!(600)
}

pub async fn handle_outbound_migration(
    vm: &VirtualMachine,
    ctx: Arc<State>,
) -> Result<Action, Error> {
    let vm_name = get_domain_name(vm).expect("VM has a libvirt domain name");
    let domain =
        Domain::lookup_by_name(&ctx.libvirt.connection, &vm_name).expect("Domain not found");
    let destination_node = vm.try_status()?.node.as_ref().expect("No destination node");

    domain.migrate_to_uri(
        &format!("qemu+ssh://{destination_node}/system"),
        virt::sys::VIR_MIGRATE_PEER2PEER
            | virt::sys::VIR_MIGRATE_LIVE
            | virt::sys::VIR_MIGRATE_AUTO_CONVERGE,
        NO_BW_LIMIT,
    )?;
    ok_and_requeue!(10)
}

pub async fn handle_inbound_migration(
    vm: &VirtualMachine,
    ctx: Arc<State>,
) -> Result<Action, Error> {
    let libvirt_domain_name = get_domain_name(vm).expect("failed to get domain name");
    let vm_runs_on_us = ctx.libvirt.has_domain(&libvirt_domain_name)?;
    if !vm_runs_on_us {
        return ok_and_requeue!(5);
    }

    let is_active = {
        Domain::lookup_by_name(&ctx.libvirt.connection, &libvirt_domain_name)
            .expect("Domain not found")
            .is_active()?
    };

    if !is_active {
        return ok_and_requeue!(5);
    }

    let mut new_status = vm.try_status()?.clone();
    new_status.migration_pending = false;
    set_vm_status(vm, new_status, ctx.kube.clone()).await?;

    ok_and_requeue!(600)
}
