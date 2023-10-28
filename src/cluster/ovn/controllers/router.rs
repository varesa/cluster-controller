use kube::runtime::controller::Action;
use kube::{
    api::{Api, PostParams},
    Client, Resource, ResourceExt,
};
use serde_json::json;
use std::sync::Arc;
use tokio::time::Duration;

use crate::cluster::ovn::logicalswitch::LogicalSwitch;
use crate::cluster::ovn::{
    common::OvnBasicActions, common::OvnNamed, common::OvnNamedGetters,
    logicalrouter::LogicalRouter, lowlevel::Ovn,
};
use crate::crd::ovn::{Route, Router, RouterStatus};
use crate::errors::Error;
use crate::metadataservice::deployment::deploy as deploy_mds;
use crate::utils::extend_traits::ExtendResource;
use crate::utils::resource_controller::{DefaultState, ResourceControllerBuilder};
use crate::{create_set_status, ok_and_requeue, ok_no_requeue};

create_set_status!(Router, RouterStatus, set_router_status);

fn delete_router(name: &str) -> Result<(), Error> {
    let ovn = Arc::new(Ovn::new("10.4.3.1", 6641));
    LogicalRouter::get_by_name(ovn, name)?.delete()
}

async fn connect_metadataservice(lr: &mut LogicalRouter) -> Result<(), Error> {
    let ovn = Arc::new(Ovn::new("10.4.3.1", 6641));
    let mds_name = format!("mds-{}", lr.name());
    let mut ls = LogicalSwitch::create_if_missing(ovn, &mds_name)?;
    super::connect_router_to_ls(lr, &mut ls, "169.254.169.253/30")?;

    ls.lsp()
        .create_if_missing(&mds_name, None)?
        .set_address("02:00:00:00:00:02")?;
    Ok(())
}

fn ensure_router_routes(router: &str, routes: &[Route]) -> Result<(), Error> {
    let ovn = Arc::new(Ovn::new("10.4.3.1", 6641));
    LogicalRouter::get_by_name(ovn, router)?.set_routes(routes)
}

/// Handle updates to routers in the cluster
async fn update_router(router: Arc<Router>, ctx: Arc<DefaultState>) -> Result<Action, Error> {
    let mut router = (*router).clone();
    let ovn = Arc::new(Ovn::new("10.4.3.1", 6641));
    let client = ctx.client.clone();
    let namespace = router
        .metadata
        .namespace
        .as_ref()
        .expect("get router namespace")
        .clone();
    let name = router.name_prefixed_with_namespace();

    println!("ovn: update for router {name}");
    router
        .ensure_finalizer("ovn", client.clone(), &super::FIELD_MANAGER)
        .await?;

    let mut lr = LogicalRouter::create_if_missing(ovn, &name)?;
    if let Some(routes) = &router.spec.routes {
        ensure_router_routes(&name, routes)?;
    } else {
        ensure_router_routes(&name, &[])?;
    }

    if let Some(true) = &router.spec.metadata_service {
        deploy_mds(
            client.clone(),
            "ovn-cluster-controller",
            &namespace,
            router.metadata.name.as_ref().expect("get router name"),
        )
        .await?;

        connect_metadataservice(&mut lr).await?;
    }

    println!("ovn: update for router {name} successful");

    let status = RouterStatus { is_created: true };
    set_router_status(&router, status, client.clone()).await?;

    ok_and_requeue!(600)
}

/// Handle updates to routers in the cluster
async fn remove_router(router: Arc<Router>, ctx: Arc<DefaultState>) -> Result<Action, Error> {
    let mut router = (*router).clone();
    let client = ctx.client.clone();
    let name = router.name_prefixed_with_namespace();

    println!("ovn: Router {} waiting for deletion", name);
    delete_router(&name)?;
    router
        .remove_finalizer("ovn", client, &super::FIELD_MANAGER)
        .await?;
    println!("ovn: Router {} deleted", name);

    ok_no_requeue!()
}

pub async fn create(client: Client) -> Result<(), Error> {
    println!("ovn.router: Starting controller");
    ResourceControllerBuilder::new(client)
        .with_default_state()
        .with_default_error_policy()
        .with_functions(update_router, remove_router)
        .run()
        .await;
    Ok(())
}
