use crate::crd::libvirt::{
    set_vm_status,
    v1beta2::{VirtualMachine, VirtualMachineStatus},
};
use crate::host::libvirt::controller::State;
use crate::host::libvirt::utils::{get_cluster, get_domain_name};
use crate::Error;
use crate::{
    api_replace_resource, client_ensure_finalizer, client_remove_finalizer, ok_and_requeue,
    ok_no_requeue, resource_has_finalizer, GROUP_NAME,
};
use kube::api::{Api, PostParams};
use kube::runtime::controller::Action;
use std::sync::Arc;
use tokio::time::Duration;
use virt::domain::Domain;

pub const LIBVIRT_URI: &str = "qemu:///system";
const NO_BW_LIMIT: u64 = 0;

pub async fn handle_delete(vm: &VirtualMachine, ctx: Arc<State>) -> Result<Action, Error> {
    let vm_name = get_domain_name(vm).expect("VM has a libvirt domain name");
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

    client_remove_finalizer!(ctx.kube.clone(), VirtualMachine, vm, "libvirt-host");

    ok_no_requeue!()
}

pub async fn handle_add(vm: &VirtualMachine, ctx: Arc<State>) -> Result<Action, Error> {
    let vm_name = get_domain_name(vm).expect("VM has a libvirt domain name");
    client_ensure_finalizer!(ctx.kube.clone(), VirtualMachine, vm, "libvirt-host");

    // Get cluster capabilities / definition
    let cluster = get_cluster(&ctx).await?;

    ctx.libvirt.create_domain(vm, &cluster)?;

    let status = VirtualMachineStatus {
        running: true,
        ..vm.status.clone().expect("VM didn't have existing status")
    };
    set_vm_status(vm, status, ctx.kube.clone()).await?;

    println!("Updated: {}", vm_name);
    ok_and_requeue!(600)
}

pub async fn handle_migration(vm: &VirtualMachine, ctx: Arc<State>) -> Result<Action, Error> {
    let vm_name = get_domain_name(vm).expect("VM has a libvirt domain name");
    let domain =
        Domain::lookup_by_name(&ctx.libvirt.connection, &vm_name).expect("Domain not found");
    let destination_node = vm
        .status
        .as_ref()
        .expect("VM has no status")
        .node
        .as_ref()
        .expect("No destination node");

    domain.migrate_to_uri(
        &format!("qemu+ssh://{destination_node}/system"),
        virt::domain::VIR_MIGRATE_PEER2PEER,
        NO_BW_LIMIT,
    )?;
    ok_and_requeue!(10)
}
