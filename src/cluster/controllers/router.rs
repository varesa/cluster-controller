use kube::runtime::controller::Action;
use kube::{
    Client, Resource, ResourceExt,
    api::{Api, PostParams},
};
use lazy_static::lazy_static;
use serde_json::json;
use std::sync::Arc;
use tokio::time::Duration;
use tracing::{info, instrument};

use crate::cluster::ovn::types::logicalswitch::LogicalSwitch;
use crate::cluster::ovn::utils::connect_router_to_ls;
use crate::cluster::ovn::{
    common::OvnBasicActions, common::OvnNamed, common::OvnNamedGetters, lowlevel::Ovn,
    types::logicalrouter::LogicalRouter,
};
use crate::crd::ovn::{Router, RouterStatus};
use crate::errors::Error;
use crate::metadataservice::deployment::deploy as deploy_mds;
use crate::utils::resource_controller::{DefaultState, ResourceControllerBuilder};
use crate::utils::strings::field_manager;
use crate::utils::traits::kube::ExtendResource;
use crate::{create_set_status_namespaced, ok_and_requeue, ok_no_requeue};

lazy_static! {
    static ref FIELD_MANAGER: String = field_manager("ovn");
}

create_set_status_namespaced!(Router, RouterStatus, set_router_status);

/// Create network components for the MDS:
/// - An LS that is connected to the router and has the MDS subnet
/// - An LSP that the MDS will use
#[instrument]
async fn connect_metadataservice(lr: &mut LogicalRouter, ovn: Arc<Ovn>) -> Result<(), Error> {
    let mds_name = format!("mds-{}", lr.name());
    let mut ls = LogicalSwitch::create_if_missing(ovn, &mds_name)?;
    connect_router_to_ls(lr, &mut ls, "169.254.169.253/30")?;

    ls.lsp()
        .create_if_missing(&mds_name, None)?
        .set_address("02:00:00:00:00:02")?;
    Ok(())
}

/// Handle updates to routers in the cluster
#[instrument(skip(ctx))]
async fn update_router(router: Arc<Router>, ctx: Arc<DefaultState>) -> Result<Action, Error> {
    let mut router = (*router).clone();
    let ovn = Arc::new(Ovn::try_from_annotations(ctx.client.clone()).await?);
    let client = ctx.client.clone();
    let namespace = router
        .metadata
        .namespace
        .as_ref()
        .expect("get router namespace")
        .clone();
    let name = router.name_prefixed_with_namespace();

    info!("ovn: update for router {name}");
    router
        .ensure_finalizer("ovn", client.clone(), &FIELD_MANAGER)
        .await?;

    let mut lr = LogicalRouter::create_if_missing(ovn.clone(), &name)?;
    if let Some(routes) = &router.spec.routes {
        LogicalRouter::get_by_name(ovn.clone(), &name)?.set_routes(routes)?
    } else {
        LogicalRouter::get_by_name(ovn.clone(), &name)?.set_routes(&[])?
    }

    if let Some(true) = &router.spec.metadata_service {
        deploy_mds(
            client.clone(),
            "ovn-cluster-controller",
            &namespace,
            router.metadata.name.as_ref().expect("get router name"),
        )
        .await?;

        connect_metadataservice(&mut lr, ovn.clone()).await?;
    }

    info!("ovn: update for router {name} successful");

    let status = RouterStatus { is_created: true };
    set_router_status(&router, status, client.clone()).await?;

    ok_and_requeue!(600)
}

/// Handle updates to routers in the cluster
#[instrument(skip(ctx))]
async fn remove_router(router: Arc<Router>, ctx: Arc<DefaultState>) -> Result<Action, Error> {
    let ovn = Arc::new(Ovn::try_from_annotations(ctx.client.clone()).await?);
    let mut router = (*router).clone();
    let client = ctx.client.clone();
    let name = router.name_prefixed_with_namespace();

    info!("ovn: Router {} waiting for deletion", name);
    LogicalRouter::get_by_name(ovn, &name)?.delete()?;
    router
        .remove_finalizer("ovn", client, &FIELD_MANAGER)
        .await?;
    info!("ovn: Router {} deleted", name);

    ok_no_requeue!()
}

#[instrument(skip(client))]
pub async fn create(client: Client) -> Result<(), Error> {
    info!("ovn.router: Starting controller");
    ResourceControllerBuilder::new(client)
        .with_default_state()
        .with_default_error_policy()
        .with_functions(update_router, remove_router)
        .run()
        .await;
    Ok(())
}
