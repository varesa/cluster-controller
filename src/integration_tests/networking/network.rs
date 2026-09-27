use super::{AbortOnDrop, ovsdb_column, ovsdb_string_map, wait_for};
use crate::{
    cluster::controllers::network,
    crd::network::{
        DhcpOptions,
        v1beta1::{Network, NetworkSpec},
    },
    integration_tests::harness::{ControlPlane, TestResult, ovn::OvnNorthbound},
};
use kube::{
    Api,
    api::{DeleteParams, PostParams},
};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

const NETWORK_NAME: &str = "tenant";
const DHCP_CIDR: &str = "10.84.0.0/24";

struct NetworkControllerTest {
    _controller: AbortOnDrop<Result<(), crate::errors::Error>>,
    plane: ControlPlane,
    ovn: OvnNorthbound,
}

impl NetworkControllerTest {
    async fn start(test_name: &str) -> TestResult<Self> {
        let plane = ControlPlane::start(test_name).await?;
        let ovn = OvnNorthbound::start(plane.client()).await?;
        let controller = AbortOnDrop(tokio::spawn(network::create(plane.client())));
        Ok(Self {
            _controller: controller,
            plane,
            ovn,
        })
    }

    fn networks(&self) -> Api<Network> {
        Api::namespaced(self.plane.client(), self.plane.namespace())
    }

    fn switch_name(&self) -> String {
        format!("{}-{NETWORK_NAME}", self.plane.namespace())
    }

    async fn create_network(&self, dhcp: Option<DhcpOptions>) -> TestResult {
        self.networks()
            .create(
                &PostParams::default(),
                &Network::new(
                    NETWORK_NAME,
                    NetworkSpec {
                        dhcp,
                        ..Default::default()
                    },
                ),
            )
            .await?;
        Ok(())
    }

    async fn switch_rows(&self, columns: &[&str]) -> TestResult<Vec<Map<String, Value>>> {
        let condition = format!("name={}", self.switch_name());
        self.ovn
            .find("Logical_Switch", columns, &[&condition])
            .await
    }

    async fn dhcp_rows(&self) -> TestResult<Vec<Map<String, Value>>> {
        let condition = format!("cidr={DHCP_CIDR}");
        self.ovn
            .find("DHCP_Options", &["cidr", "options"], &[&condition])
            .await
    }

    async fn all_dhcp_rows(&self) -> TestResult<Vec<Map<String, Value>>> {
        self.ovn.find("DHCP_Options", &["cidr"], &[]).await
    }
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn ovn_network_creates_logical_switch() -> TestResult {
    let test = NetworkControllerTest::start("ovn_network_creates_logical_switch").await?;
    test.create_network(None).await?;

    wait_for("logical switch creation", || async {
        Ok(test.switch_rows(&["name"]).await?.len() == 1)
    })
    .await?;

    assert_eq!(test.switch_rows(&["name"]).await?.len(), 1);
    Ok(())
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn ovn_network_without_dhcp_leaves_dhcp_unconfigured() -> TestResult {
    let test =
        NetworkControllerTest::start("ovn_network_without_dhcp_leaves_dhcp_unconfigured").await?;
    test.create_network(None).await?;
    wait_for("logical switch creation", || async {
        Ok(test.switch_rows(&["name"]).await?.len() == 1)
    })
    .await?;

    let switches = test.switch_rows(&["other_config"]).await?;
    assert_eq!(switches.len(), 1);
    assert!(!ovsdb_string_map(ovsdb_column(&switches[0], "other_config")?)?.contains_key("subnet"));
    assert!(test.all_dhcp_rows().await?.is_empty());
    Ok(())
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn ovn_network_configures_dhcp() -> TestResult {
    let test = NetworkControllerTest::start("ovn_network_configures_dhcp").await?;
    test.create_network(Some(DhcpOptions {
        cidr: DHCP_CIDR.into(),
        lease_time: Some(3600),
        dns_server: Some("10.84.0.53".into()),
        domain_name: Some("tenant.example".into()),
        router: Some("10.84.0.1".into()),
    }))
    .await?;

    let expected_options = BTreeMap::from([
        ("dns_server".into(), "10.84.0.53".into()),
        ("domain_name".into(), "\"tenant.example\"".into()),
        ("lease_time".into(), "3600".into()),
        ("router".into(), "10.84.0.1".into()),
        ("server_id".into(), "10.84.0.1".into()),
        ("server_mac".into(), "c0:ff:ee:00:00:01".into()),
    ]);
    wait_for("DHCP options configuration", || async {
        let rows = test.dhcp_rows().await?;
        if rows.len() != 1 {
            return Ok(false);
        }
        Ok(ovsdb_string_map(ovsdb_column(&rows[0], "options")?)? == expected_options)
    })
    .await?;

    let switches = test.switch_rows(&["other_config"]).await?;
    assert_eq!(switches.len(), 1);
    assert_eq!(
        ovsdb_string_map(ovsdb_column(&switches[0], "other_config")?)?
            .get("subnet")
            .map(String::as_str),
        Some(DHCP_CIDR)
    );
    let rows = test.dhcp_rows().await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        ovsdb_string_map(ovsdb_column(&rows[0], "options")?)?,
        expected_options
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn ovn_network_deletes_logical_switch() -> TestResult {
    let test = NetworkControllerTest::start("ovn_network_deletes_logical_switch").await?;
    test.create_network(None).await?;
    wait_for("logical switch creation", || async {
        Ok(test.switch_rows(&["name"]).await?.len() == 1)
    })
    .await?;

    test.networks()
        .delete(NETWORK_NAME, &DeleteParams::default())
        .await?;
    wait_for("logical switch deletion", || async {
        Ok(test.switch_rows(&["name"]).await?.is_empty())
    })
    .await?;

    assert!(test.switch_rows(&["name"]).await?.is_empty());
    Ok(())
}
