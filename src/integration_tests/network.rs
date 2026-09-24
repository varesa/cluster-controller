use super::harness::{ControlPlane, TestResult, ovn::OvnNorthbound};

use std::{sync::Arc, time::Duration};

use kube::{
    Api,
    api::{DeleteParams, PostParams},
};
use serde_json::{Value, json};
use tokio::{
    task::JoinHandle,
    time::{sleep, timeout},
};

use crate::{
    cluster::controllers::network,
    crd::network::{
        DhcpOptions, NetworkType,
        v1beta1::{Network, NetworkSpec},
    },
    errors::Error,
    interfaces::ovn::{
        common::OvnNamedGetters, lowlevel::Ovn, types::logicalswitch::LogicalSwitch,
    },
};

struct AbortOnDrop(JoinHandle<Result<(), Error>>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn dhcp_options_configured(ovn: &Ovn) -> bool {
    let result = ovn.transact(&[json!({
        "op": "select",
        "table": "DHCP_Options",
        "where": [["cidr", "==", "10.84.0.0/24"]],
        "columns": ["options"],
    })]);
    let entries = result
        .first()
        .and_then(|result| result.get("rows"))
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("options"))
        .and_then(Value::as_array)
        .and_then(|map| map.get(1))
        .and_then(Value::as_array);
    entries.is_some_and(|entries| {
        entries.contains(&json!(["lease_time", "3600"]))
            && entries.contains(&json!(["dns_server", "10.84.0.53"]))
    })
}

#[tokio::test]
#[cfg(feature = "control-plane-tests")]
#[ignore = "requires control plane"]
async fn ovn_network_creates_switch_configures_dhcp_and_removes_switch_before_deletion()
-> TestResult {
    let plane = ControlPlane::start(
        "ovn_network_creates_switch_configures_dhcp_and_removes_switch_before_deletion",
    )
    .await?;
    let _backend = OvnNorthbound::start(plane.client()).await?;
    let ovn = Arc::new(Ovn::try_new("127.0.0.1", 6641)?);
    let networks: Api<Network> = Api::namespaced(plane.client(), plane.namespace());
    let switch_name = format!("{}-tenant", plane.namespace());
    let _controller = AbortOnDrop(tokio::spawn(network::create(plane.client())));

    networks
        .create(
            &PostParams::default(),
            &Network::new(
                "tenant",
                NetworkSpec {
                    network_type: Some(NetworkType::Ovn),
                    dhcp: Some(DhcpOptions {
                        cidr: "10.84.0.0/24".into(),
                        lease_time: Some(3600),
                        dns_server: Some("10.84.0.53".into()),
                        domain_name: None,
                        router: None,
                    }),
                    ..Default::default()
                },
            ),
        )
        .await?;

    timeout(Duration::from_secs(30), async {
        loop {
            let current = networks.get("tenant").await?;
            let switch = LogicalSwitch::get_by_name(ovn.clone(), &switch_name);
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
                && switch
                    .as_ref()
                    .is_ok_and(|switch| switch.get_cidr().as_deref() == Some("10.84.0.0/24"))
                && dhcp_options_configured(&ovn)
            {
                break Ok::<(), Box<dyn std::error::Error + Send + Sync>>(());
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await??;

    networks.delete("tenant", &DeleteParams::default()).await?;
    timeout(Duration::from_secs(30), async {
        loop {
            if networks.get_opt("tenant").await?.is_none()
                && matches!(
                    LogicalSwitch::get_by_name(ovn.clone(), &switch_name),
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
