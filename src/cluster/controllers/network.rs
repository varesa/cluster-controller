use kube::runtime::controller::Action;
use kube::{
    api::{Api, PostParams}, Client, Resource,
    ResourceExt,
};
use lazy_static::lazy_static;
use serde_json::json;
use std::sync::Arc;
use tokio::time::Duration;
use tracing::{info, instrument};

use crate::crd::network::{
    DhcpOptions as DhcpOptionsCrd, Network, NetworkStatus, NetworkType, RouterAttachment,
};
use crate::errors::Error;
use crate::interfaces::ovn::types::logicalswitch::LogicalSwitch;
use crate::interfaces::ovn::utils::connect_router_to_ls;
use crate::interfaces::ovn::{
    common::OvnBasicActions, common::OvnNamedGetters, lowlevel::Ovn,
    types::dhcpoptions::DhcpOptions, types::logicalrouter::LogicalRouter,
};
use crate::utils::resource_controller::{DefaultState, ResourceControllerBuilder};
use crate::utils::strings::field_manager;
use crate::utils::traits::kube::ExtendResource;
use crate::{create_set_status_namespaced, ok_and_requeue, ok_no_requeue};

lazy_static! {
    static ref FIELD_MANAGER: String = field_manager("ovn");
}

create_set_status_namespaced!(Network, NetworkStatus, set_network_status);

/// Attempts to
/// - configure an DHCP Option set for the prefix
/// - Apply the DHCP prefix to the LS
///
/// no-op if already set correctly
#[instrument]
fn ensure_dhcp(name: &str, dhcp: &DhcpOptionsCrd, ovn: Arc<Ovn>) -> Result<(), Error> {
    let mut dhcp_opts = match DhcpOptions::get_by_cidr(ovn.clone(), &dhcp.cidr) {
        Ok(opts) => Ok(opts),
        Err(Error::OvnNotFound(_, _)) => DhcpOptions::create(ovn.clone(), &dhcp.cidr),
        Err(e) => Err(e),
    }?;

    // Lazily try to always update, effectively noop if current value are already correct
    dhcp_opts.set_options(dhcp)?;

    LogicalSwitch::get_by_name(ovn, name)?.set_cidr(&dhcp.cidr)?;
    Ok(())
}

/// Connect a router to a logical switch with the given IP address
#[instrument]
fn ensure_router_attachment(
    network: &Network,
    router_attachment: &RouterAttachment,
    ovn: Arc<Ovn>,
) -> Result<(), Error> {
    let network_ns = ResourceExt::namespace(network).expect("Get network ns");

    let split: Vec<String> = router_attachment
        .name
        .split('/')
        .map(String::from)
        .collect();
    let (namespace, name) = match split.len() {
        1 => (&network_ns, split.first().unwrap()),
        2 => (split.first().unwrap(), split.get(1).unwrap()),
        _ => panic!("Malformed router name (todo: error)"),
    };

    let lr_name = format!("{}-{}", &namespace, &name);
    let mut lr = LogicalRouter::get_by_name(ovn.clone(), &lr_name)?;

    let ls_name = network.name_prefixed_with_namespace();
    let mut ls = LogicalSwitch::get_by_name(ovn, &ls_name)?;

    connect_router_to_ls(&mut lr, &mut ls, &router_attachment.address)?;

    Ok(())
}

/// Handle updates to networks in the cluster
#[instrument(skip(ctx))]
async fn update_network(network: Arc<Network>, ctx: Arc<DefaultState>) -> Result<Action, Error> {
    let mut network = (*network).clone();

    // We are only interested in OVN networks. Ignore other types
    if network.spec.network_type.clone().unwrap_or_default() != NetworkType::Ovn {
        return ok_no_requeue!();
    }

    let name = network.name_prefixed_with_namespace();
    info!("ovn: update for network {name}");

    let ovn = Arc::new(Ovn::try_from_annotations(ctx.client.clone()).await?);
    let client = ctx.client.clone();

    network
        .ensure_finalizer("ovn", client.clone(), &FIELD_MANAGER)
        .await?;
    LogicalSwitch::create_if_missing(ovn.clone(), &name)?;
    if let Some(dhcp_options) = network.spec.dhcp.as_ref() {
        ensure_dhcp(&name, dhcp_options, ovn.clone())?;
    }

    if let Some(routers) = network.spec.routers.as_ref() {
        for router in routers {
            ensure_router_attachment(&network, router, ovn.clone())?;
        }
    }

    info!("ovn: update for network {name} successful");

    let status = NetworkStatus { is_created: true };
    set_network_status(&network, status, client.clone()).await?;

    ok_and_requeue!(600)
}

/// Handle updates to networks in the cluster
#[instrument(skip(ctx))]
async fn remove_network(network: Arc<Network>, ctx: Arc<DefaultState>) -> Result<Action, Error> {
    let mut network = (*network).clone();

    // We are only interested in OVN networks. Ignore other types
    if network.spec.network_type.clone().unwrap_or_default() != NetworkType::Ovn {
        return ok_no_requeue!();
    }

    let ovn = Arc::new(Ovn::try_from_annotations(ctx.client.clone()).await?);
    let client = ctx.client.clone();
    let name = network.name_prefixed_with_namespace();

    info!("ovn: Network {} waiting for deletion", name);
    let ls = LogicalSwitch::get_by_name(ovn, &name);
    match ls {
        Ok(ls) => ls.delete()?,
        Err(Error::OvnNotFound(_, _)) => { /* nothing to delete */ }
        Err(e) => return Err(e),
    };

    network
        .remove_finalizer("ovn", client, &FIELD_MANAGER)
        .await?;
    info!("ovn: Network {} deleted", name);

    ok_and_requeue!(600)
}

#[instrument(skip(client))]
pub async fn create(client: Client) -> Result<(), Error> {
    info!("ovn.network: Starting controller");
    ResourceControllerBuilder::new(client)
        .with_default_state()
        .with_default_error_policy()
        .with_functions(update_network, remove_network)
        .run()
        .await;
    Ok(())
}
