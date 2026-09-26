use crate::integration_tests::harness::{ControlPlane, TestResult, ovn::OvnNorthbound};

use std::{sync::Arc, time::Duration};

use kube::{
    Api,
    api::{DeleteParams, Patch, PatchParams, PostParams},
};
use serde_json::json;
use tokio::{
    task::JoinHandle,
    time::{sleep, timeout},
};

use crate::{
    cluster::controllers::router,
    crd::router::{Route, Router, RouterSpec},
    errors::Error,
    interfaces::ovn::{
        common::OvnNamedGetters, lowlevel::Ovn, types::logicalrouter::LogicalRouter,
    },
};

struct AbortOnDrop(JoinHandle<Result<(), Error>>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn ovn_router_reconciles_routes_and_deletes_router() -> TestResult {
    let plane = ControlPlane::start("ovn_router_reconciles_routes_and_deletes_router").await?;
    let _backend = OvnNorthbound::start(plane.client()).await?;
    let ovn = Arc::new(Ovn::try_new("127.0.0.1", 6641)?);
    let routers: Api<Router> = Api::namespaced(plane.client(), plane.namespace());
    let router_name = format!("{}-gateway", plane.namespace());
    let _controller = AbortOnDrop(tokio::spawn(router::create(plane.client())));

    routers
        .create(
            &PostParams::default(),
            &Router::new(
                "gateway",
                RouterSpec {
                    routes: Some(vec![Route {
                        cidr: "10.42.0.0/16".into(),
                        nexthop: "192.0.2.1".into(),
                    }]),
                    metadata_service: None,
                },
            ),
        )
        .await?;

    timeout(Duration::from_secs(30), async {
        loop {
            let current = routers.get("gateway").await?;
            let routes = LogicalRouter::get_by_name(ovn.clone(), &router_name)
                .and_then(|router| router.get_routes());
            if current
                .status
                .as_ref()
                .is_some_and(|status| status.is_created)
                && current
                    .metadata
                    .finalizers
                    .as_ref()
                    .is_some_and(|finalizers| {
                        finalizers
                            .iter()
                            .any(|name| name == "cluster-virt.acl.fi/ovn")
                    })
                && routes.as_ref().is_ok_and(|routes| {
                    routes.len() == 1
                        && routes[0].ip_prefix == "10.42.0.0/16"
                        && routes[0].nexthop == "192.0.2.1"
                })
            {
                break Ok::<(), Box<dyn std::error::Error + Send + Sync>>(());
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await??;

    routers
        .patch(
            "gateway",
            &PatchParams::default(),
            &Patch::Merge(json!({
                "spec": {"routes": [{"cidr": "10.43.0.0/16", "nexthop": "192.0.2.2"}]}
            })),
        )
        .await?;
    timeout(Duration::from_secs(30), async {
        loop {
            let routes = LogicalRouter::get_by_name(ovn.clone(), &router_name)
                .and_then(|router| router.get_routes());
            if routes.as_ref().is_ok_and(|routes| {
                routes.len() == 1
                    && routes[0].ip_prefix == "10.43.0.0/16"
                    && routes[0].nexthop == "192.0.2.2"
            }) {
                break Ok::<(), Box<dyn std::error::Error + Send + Sync>>(());
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await??;

    routers.delete("gateway", &DeleteParams::default()).await?;
    timeout(Duration::from_secs(30), async {
        loop {
            if routers.get_opt("gateway").await?.is_none()
                && matches!(
                    LogicalRouter::get_by_name(ovn.clone(), &router_name),
                    Err(Error::OvnNotFound(_, _))
                )
            {
                break Ok::<(), Box<dyn std::error::Error + Send + Sync>>(());
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await??;
    Ok(())
}
