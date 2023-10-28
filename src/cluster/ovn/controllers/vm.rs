use std::sync::Arc;

use kube::runtime::controller::Action;
use kube::{api::Api, Client, ResourceExt};
use tokio::time::Duration;

use crate::cluster::ovn::types::logicalswitch::LogicalSwitch;
use crate::cluster::ovn::{
    common::OvnNamedGetters, lowlevel::Ovn, types::logicalswitchport::LogicalSwitchPort,
};
use crate::crd::libvirt::{set_vm_status, NetworkAttachment, VirtualMachine, VirtualMachineStatus};
use crate::crd::ovn::Network;
use crate::errors::Error;
use crate::utils::extend_traits::ExtendResource;
use crate::utils::resource_controller::{DefaultState, ResourceControllerBuilder};
use crate::{ok_and_requeue, ok_no_requeue};

async fn connect_vm_nic(
    client: Client,
    vm: &VirtualMachine,
    nic: &NetworkAttachment,
) -> Result<(), Error> {
    let ovn = Arc::new(Ovn::new("10.4.3.1", 6641));
    let namespace = ResourceExt::namespace(vm).expect("Failed to get VM namespace");
    let network_name = nic.name.as_ref().expect("No network name set");
    let mac_address = nic.mac_address.as_ref().expect("MAC address missing");

    let ls_name = format!("{}-{}", &namespace, &network_name);
    let mut ls = LogicalSwitch::get_by_name(ovn.clone(), &ls_name)?;

    let lsp_id = nic.ovn_id.as_ref().unwrap();
    let mut lsp = ls.lsp().create_if_missing(lsp_id, None)?;

    lsp.set_address(mac_address)?;

    let api: Api<Network> = Api::namespaced(client.clone(), &namespace);
    let network = api.get(network_name).await?;
    if let Some(dhcp) = network.spec.dhcp {
        lsp.set_dhcp_options(&dhcp.cidr)?;
    }
    Ok(())
}

fn disconnect_vm_nic(vm: &VirtualMachine, nic: &NetworkAttachment) -> Result<(), Error> {
    let ovn = Arc::new(Ovn::new("10.4.3.1", 6641));
    let ls_name = format!(
        "{}-{}",
        ResourceExt::namespace(vm).expect("Failed to get VM namespace"),
        nic.name.as_ref().expect("No network name set")
    );
    if LogicalSwitchPort::get_by_name(ovn.clone(), nic.ovn_id.as_ref().unwrap()).is_ok() {
        println!("ovn: lsp exists for NIC, removing");
        let mut ls = LogicalSwitch::get_by_name(ovn, &ls_name)?;
        ls.del_lsp(nic.ovn_id.as_ref().unwrap())?;
    }
    Ok(())
}

fn get_vm_ovn_nics(vm: &VirtualMachine) -> Vec<NetworkAttachment> {
    match &vm.status {
        Some(status) => status
            .networks
            .clone()
            .into_iter()
            .filter(|net| net.ovn_id.is_some())
            .collect(),
        None => Vec::new(),
    }
}

/// Handle updates to VMs in the cluster
async fn update_vm(vm: Arc<VirtualMachine>, ctx: Arc<DefaultState>) -> Result<Action, Error> {
    let mut vm = (*vm).clone();
    let name = vm.name_prefixed_with_namespace();
    let client = ctx.client.clone();

    println!("ovn: VM {name} updated");
    vm.ensure_finalizer("ovn", client.clone(), &super::FIELD_MANAGER)
        .await?;
    let mut ip_addresses: Vec<String> = Vec::new();
    for (index, nic) in get_vm_ovn_nics(&vm).iter().enumerate() {
        println!("ovn: connecting NIC {index} for VM {name}");
        connect_vm_nic(client.clone(), &vm, nic).await?;
        if let Some(ovn_id) = nic.ovn_id.as_ref() {
            let ovn = Arc::new(Ovn::new("10.4.3.1", 6641));
            let lsp = LogicalSwitchPort::get_by_name(ovn, ovn_id)?;
            if let Some(ip) = lsp.dynamic_ip() {
                ip_addresses.push(ip);
            }
        }
    }

    if let Some(status) = vm.status.clone() {
        let ip_addresses_string = ip_addresses.join(",");
        let new_status = VirtualMachineStatus {
            ip_addresses: Some(ip_addresses),
            ip_addresses_string: Some(ip_addresses_string),
            ..status
        };
        set_vm_status(&vm, new_status, client).await?;
    }

    ok_and_requeue!(600)
}

/// Handle updates to VMs in the cluster
async fn remove_vm(vm: Arc<VirtualMachine>, ctx: Arc<DefaultState>) -> Result<Action, Error> {
    let mut vm = (*vm).clone();
    let name = vm.name_prefixed_with_namespace();
    let client = ctx.client.clone();

    println!("ovn: VM {name} waiting for deletion");
    for (index, nic) in get_vm_ovn_nics(&vm).iter().enumerate() {
        println!("ovn: disconnecting NIC {index} for VM {name}");
        disconnect_vm_nic(&vm, nic)?;
    }
    vm.remove_finalizer("ovn", client, &super::FIELD_MANAGER)
        .await?;
    ok_no_requeue!()
}

pub async fn create(client: Client) -> Result<(), Error> {
    println!("ovn.vm: Starting controller");
    ResourceControllerBuilder::new(client)
        .with_default_state()
        .with_default_error_policy()
        .with_functions(update_vm, remove_vm)
        .run()
        .await;
    Ok(())
}
