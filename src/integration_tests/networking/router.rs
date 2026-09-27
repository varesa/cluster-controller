use super::{AbortOnDrop, ovsdb_column, ovsdb_uuid, ovsdb_uuid_set, wait_for};
use crate::{
    cluster::controllers::router,
    crd::router::{Route, Router, RouterSpec},
    integration_tests::harness::{ControlPlane, TestResult, ovn::OvnNorthbound},
};
use kube::{
    Api,
    api::{DeleteParams, Patch, PatchParams, PostParams},
};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

const ROUTER_NAME: &str = "gateway";
const FIRST_CIDR: &str = "10.42.0.0/16";
const FIRST_NEXTHOP: &str = "192.0.2.1";
const SECOND_CIDR: &str = "10.43.0.0/16";
const SECOND_NEXTHOP: &str = "192.0.2.2";

struct RouterControllerTest {
    _controller: AbortOnDrop<Result<(), crate::errors::Error>>,
    plane: ControlPlane,
    ovn: OvnNorthbound,
}

impl RouterControllerTest {
    async fn start(test_name: &str) -> TestResult<Self> {
        let plane = ControlPlane::start(test_name).await?;
        let ovn = OvnNorthbound::start(plane.client()).await?;
        let controller = AbortOnDrop(tokio::spawn(router::create(plane.client())));
        Ok(Self {
            _controller: controller,
            plane,
            ovn,
        })
    }

    fn routers(&self) -> Api<Router> {
        Api::namespaced(self.plane.client(), self.plane.namespace())
    }

    fn router_name(&self) -> String {
        format!("{}-{ROUTER_NAME}", self.plane.namespace())
    }

    async fn create_router(&self, routes: Option<Vec<Route>>) -> TestResult {
        self.routers()
            .create(
                &PostParams::default(),
                &Router::new(
                    ROUTER_NAME,
                    RouterSpec {
                        routes,
                        metadata_service: None,
                    },
                ),
            )
            .await?;
        Ok(())
    }

    async fn router_rows(&self, columns: &[&str]) -> TestResult<Vec<Map<String, Value>>> {
        let condition = format!("name={}", self.router_name());
        self.ovn
            .find("Logical_Router", columns, &[&condition])
            .await
    }

    async fn route_rows(&self, cidr: &str, nexthop: &str) -> TestResult<Vec<Map<String, Value>>> {
        let cidr = format!("ip_prefix={cidr}");
        let nexthop = format!("nexthop={nexthop}");
        self.ovn
            .find(
                "Logical_Router_Static_Route",
                &["_uuid", "ip_prefix", "nexthop"],
                &[&cidr, &nexthop],
            )
            .await
    }

    async fn route_is_attached(&self, cidr: &str, nexthop: &str) -> TestResult<bool> {
        let routes = self.route_rows(cidr, nexthop).await?;
        let routers = self.router_rows(&["static_routes"]).await?;
        if routes.len() != 1 || routers.len() != 1 {
            return Ok(false);
        }
        let route_id = ovsdb_uuid(ovsdb_column(&routes[0], "_uuid")?)?;
        Ok(ovsdb_uuid_set(ovsdb_column(&routers[0], "static_routes")?)?
            == BTreeSet::from([route_id.to_owned()]))
    }
}

fn route(cidr: &str, nexthop: &str) -> Route {
    Route {
        cidr: cidr.into(),
        nexthop: nexthop.into(),
    }
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn ovn_router_creates_logical_router() -> TestResult {
    let test = RouterControllerTest::start("ovn_router_creates_logical_router").await?;
    test.create_router(None).await?;

    wait_for("logical router creation", || async {
        Ok(test.router_rows(&["name"]).await?.len() == 1)
    })
    .await?;

    assert_eq!(test.router_rows(&["name"]).await?.len(), 1);
    Ok(())
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn ovn_router_adds_static_route() -> TestResult {
    let test = RouterControllerTest::start("ovn_router_adds_static_route").await?;
    test.create_router(Some(vec![route(FIRST_CIDR, FIRST_NEXTHOP)]))
        .await?;

    wait_for("static route addition", || async {
        test.route_is_attached(FIRST_CIDR, FIRST_NEXTHOP).await
    })
    .await?;

    assert!(test.route_is_attached(FIRST_CIDR, FIRST_NEXTHOP).await?);
    Ok(())
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn ovn_router_replaces_static_route() -> TestResult {
    let test = RouterControllerTest::start("ovn_router_replaces_static_route").await?;
    test.create_router(Some(vec![route(FIRST_CIDR, FIRST_NEXTHOP)]))
        .await?;
    wait_for("initial static route", || async {
        test.route_is_attached(FIRST_CIDR, FIRST_NEXTHOP).await
    })
    .await?;

    test.routers()
        .patch(
            ROUTER_NAME,
            &PatchParams::default(),
            &Patch::Merge(json!({
                "spec": {"routes": [{"cidr": SECOND_CIDR, "nexthop": SECOND_NEXTHOP}]}
            })),
        )
        .await?;
    wait_for("replacement static route", || async {
        test.route_is_attached(SECOND_CIDR, SECOND_NEXTHOP).await
    })
    .await?;

    assert!(test.route_rows(FIRST_CIDR, FIRST_NEXTHOP).await?.is_empty());
    assert!(test.route_is_attached(SECOND_CIDR, SECOND_NEXTHOP).await?);
    Ok(())
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn ovn_router_deletes_logical_router() -> TestResult {
    let test = RouterControllerTest::start("ovn_router_deletes_logical_router").await?;
    test.create_router(None).await?;
    wait_for("logical router creation", || async {
        Ok(test.router_rows(&["name"]).await?.len() == 1)
    })
    .await?;

    test.routers()
        .delete(ROUTER_NAME, &DeleteParams::default())
        .await?;
    wait_for("logical router deletion", || async {
        Ok(test.router_rows(&["name"]).await?.is_empty())
    })
    .await?;

    assert!(test.router_rows(&["name"]).await?.is_empty());
    Ok(())
}
