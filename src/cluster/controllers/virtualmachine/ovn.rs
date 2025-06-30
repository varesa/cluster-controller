use std::sync::Arc;

use kube::runtime::controller::Action;
use kube::{Client, ResourceExt, api::Api};
use lazy_static::lazy_static;
use tokio::time::Duration;
use tracing::{info, instrument};

use crate::crd::network::Network;
use crate::crd::virtualmachine::{
    NetworkAttachment, VirtualMachine, VirtualMachineStatus, set_vm_status,
};
use crate::errors::Error;
use crate::interfaces::ovn::types::logicalswitch::LogicalSwitch;
use crate::interfaces::ovn::{
    common::OvnNamedGetters, lowlevel::Ovn, types::logicalswitchport::LogicalSwitchPort,
};
use crate::utils::resource_controller::{DefaultState, ResourceControllerBuilder};
use crate::utils::strings::field_manager;
use crate::utils::traits::kube::ExtendResource;
use crate::{ok_and_requeue, ok_no_requeue};

lazy_static! {
    static ref FIELD_MANAGER: String = field_manager("vm.ovn");
}

#[instrument(skip(client))]
async fn connect_vm_nic(
    client: Client,
    vm: &VirtualMachine,
    nic: &NetworkAttachment,
    ovn: Arc<Ovn>,
) -> Result<(), Error> {
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

#[instrument]
fn disconnect_vm_nic(
    vm: &VirtualMachine,
    nic: &NetworkAttachment,
    ovn: Arc<Ovn>,
) -> Result<(), Error> {
    let ls_name = format!(
        "{}-{}",
        ResourceExt::namespace(vm).expect("Failed to get VM namespace"),
        nic.name.as_ref().expect("No network name set")
    );
    if LogicalSwitchPort::get_by_name(ovn.clone(), nic.ovn_id.as_ref().unwrap()).is_ok() {
        info!("ovn: lsp exists for NIC, removing");
        let mut ls = LogicalSwitch::get_by_name(ovn, &ls_name)?;
        ls.del_lsp(nic.ovn_id.as_ref().unwrap())?;
    }
    Ok(())
}

#[instrument]
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
#[instrument(skip(ctx))]
async fn update_vm(vm: Arc<VirtualMachine>, ctx: Arc<DefaultState>) -> Result<Action, Error> {
    let mut vm = (*vm).clone();
    let name = vm.name_prefixed_with_namespace();
    let client = ctx.client.clone();
    let ovn = Arc::new(Ovn::try_from_annotations(ctx.client.clone()).await?);

    info!("ovn: VM {name} updated");
    vm.ensure_finalizer("ovn", client.clone(), &FIELD_MANAGER)
        .await?;
    let mut ip_addresses: Vec<String> = Vec::new();
    for (index, nic) in get_vm_ovn_nics(&vm).iter().enumerate() {
        info!("ovn: connecting NIC {index} for VM {name}");
        connect_vm_nic(client.clone(), &vm, nic, ovn.clone()).await?;
        if let Some(ovn_id) = nic.ovn_id.as_ref() {
            let lsp = LogicalSwitchPort::get_by_name(ovn.clone(), ovn_id)?;
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
#[instrument(skip(ctx))]
async fn remove_vm(vm: Arc<VirtualMachine>, ctx: Arc<DefaultState>) -> Result<Action, Error> {
    let mut vm = (*vm).clone();
    let name = vm.name_prefixed_with_namespace();
    let client = ctx.client.clone();
    let ovn = Arc::new(Ovn::try_from_annotations(ctx.client.clone()).await?);

    info!("ovn: VM {name} waiting for deletion");
    for (index, nic) in get_vm_ovn_nics(&vm).iter().enumerate() {
        info!("ovn: disconnecting NIC {index} for VM {name}");
        disconnect_vm_nic(&vm, nic, ovn.clone())?;
    }
    vm.remove_finalizer("ovn", client, &FIELD_MANAGER).await?;
    ok_no_requeue!()
}

pub async fn create(client: Client) -> Result<(), Error> {
    info!("ovn.vm: Starting controller");
    ResourceControllerBuilder::new(client)
        .with_default_state()
        .with_default_error_policy()
        .with_functions(update_vm, remove_vm)
        .run()
        .await;
    Ok(())
}
