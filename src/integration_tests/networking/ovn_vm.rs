use super::{
    AbortOnDrop, ovsdb_column, ovsdb_string_map, ovsdb_string_set, ovsdb_uuid, ovsdb_uuid_set,
    wait_for,
};
use crate::{
    cluster::controllers::{network, virtualmachine::ovn as ovn_controller},
    crd::{
        network::{
            DhcpOptions,
            v1beta1::{Network, NetworkSpec},
        },
        virtualmachine::{
            NetworkAttachment,
            v1beta3::{VirtualMachine, VirtualMachineSpec, VirtualMachineStatus},
        },
    },
    integration_tests::harness::{ControlPlane, TestResult, ovn::OvnNorthbound},
};
use kube::{
    Api,
    api::{DeleteParams, Patch, PatchParams, PostParams},
};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

const NETWORK_NAME: &str = "tenant";
const VM_NAME: &str = "test-vm";
const PORT_NAME: &str = "test-vm-nic";
const MAC_ADDRESS: &str = "02:00:00:00:00:42";
const DHCP_CIDR: &str = "10.84.0.0/24";

struct OvnVmControllerTest {
    _vm_controller: AbortOnDrop<Result<(), crate::errors::Error>>,
    _network_controller: AbortOnDrop<Result<(), crate::errors::Error>>,
    plane: ControlPlane,
    ovn: OvnNorthbound,
}

impl OvnVmControllerTest {
    async fn start(test_name: &str) -> TestResult<Self> {
        let plane = ControlPlane::start(test_name).await?;
        let ovn = OvnNorthbound::start(plane.client()).await?;
        let network_controller = AbortOnDrop(tokio::spawn(network::create(plane.client())));
        let vm_controller = AbortOnDrop(tokio::spawn(ovn_controller::create(plane.client())));
        Ok(Self {
            _vm_controller: vm_controller,
            _network_controller: network_controller,
            plane,
            ovn,
        })
    }

    fn networks(&self) -> Api<Network> {
        Api::namespaced(self.plane.client(), self.plane.namespace())
    }

    fn vms(&self) -> Api<VirtualMachine> {
        Api::namespaced(self.plane.client(), self.plane.namespace())
    }

    fn switch_name(&self) -> String {
        format!("{}-{NETWORK_NAME}", self.plane.namespace())
    }

    async fn switch_rows(&self, columns: &[&str]) -> TestResult<Vec<Map<String, Value>>> {
        let condition = format!("name={}", self.switch_name());
        self.ovn
            .find("Logical_Switch", columns, &[&condition])
            .await
    }

    async fn port_rows(&self) -> TestResult<Vec<Map<String, Value>>> {
        let condition = format!("name={PORT_NAME}");
        self.ovn
            .find(
                "Logical_Switch_Port",
                &["_uuid", "name", "addresses", "dhcpv4_options"],
                &[&condition],
            )
            .await
    }

    async fn dhcp_rows(&self, cidr: &str) -> TestResult<Vec<Map<String, Value>>> {
        let condition = format!("cidr={cidr}");
        self.ovn
            .find("DHCP_Options", &["_uuid", "cidr"], &[&condition])
            .await
    }

    async fn create_network(&self, dhcp: Option<DhcpOptions>) -> TestResult {
        let dhcp_cidr = dhcp.as_ref().map(|options| options.cidr.clone());
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
        wait_for("logical switch prerequisite", || async {
            let switches = self.switch_rows(&["other_config"]).await?;
            if switches.len() != 1 {
                return Ok(false);
            }
            let Some(cidr) = dhcp_cidr.as_deref() else {
                return Ok(true);
            };
            Ok(
                ovsdb_string_map(ovsdb_column(&switches[0], "other_config")?)?
                    .get("subnet")
                    .is_some_and(|subnet| subnet == cidr)
                    && self.dhcp_rows(cidr).await?.len() == 1,
            )
        })
        .await
    }

    async fn create_vm(&self) -> TestResult {
        let nic = NetworkAttachment {
            name: Some(NETWORK_NAME.into()),
            mac_address: Some(MAC_ADDRESS.into()),
            ovn_id: Some(PORT_NAME.into()),
            ..Default::default()
        };
        self.vms()
            .create(
                &PostParams::default(),
                &VirtualMachine::new(
                    VM_NAME,
                    VirtualMachineSpec {
                        cpus: 1,
                        memory: "128 Mi".into(),
                        networks: vec![nic.clone()],
                        ..Default::default()
                    },
                ),
            )
            .await?;
        self.vms()
            .patch_status(
                VM_NAME,
                &PatchParams::default(),
                &Patch::Merge(json!({
                    "status": VirtualMachineStatus {
                        scheduled: false,
                        running: false,
                        migration_pending: false,
                        node: None,
                        domain_name: String::new(),
                        ip_addresses: None,
                        ip_addresses_string: None,
                        networks: vec![nic],
                    }
                })),
            )
            .await?;
        Ok(())
    }

    async fn nic_matches(&self, address: &str, dhcp_cidr: Option<&str>) -> TestResult<bool> {
        let ports = self.port_rows().await?;
        let switches = self.switch_rows(&["ports"]).await?;
        if ports.len() != 1 || switches.len() != 1 {
            return Ok(false);
        }

        let port_id = ovsdb_uuid(ovsdb_column(&ports[0], "_uuid")?)?;
        if ovsdb_uuid_set(ovsdb_column(&switches[0], "ports")?)?
            != BTreeSet::from([port_id.to_owned()])
            || ovsdb_string_set(ovsdb_column(&ports[0], "addresses")?)?
                != BTreeSet::from([address.to_owned()])
        {
            return Ok(false);
        }

        let expected_dhcp_ids = if let Some(cidr) = dhcp_cidr {
            let options = self.dhcp_rows(cidr).await?;
            if options.len() != 1 {
                return Ok(false);
            }
            BTreeSet::from([ovsdb_uuid(ovsdb_column(&options[0], "_uuid")?)?.to_owned()])
        } else {
            BTreeSet::new()
        };
        Ok(ovsdb_uuid_set(ovsdb_column(&ports[0], "dhcpv4_options")?)? == expected_dhcp_ids)
    }
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn ovn_vm_connects_nic_without_dhcp() -> TestResult {
    let test = OvnVmControllerTest::start("ovn_vm_connects_nic_without_dhcp").await?;
    test.create_network(None).await?;
    test.create_vm().await?;

    wait_for("logical switch port without DHCP", || async {
        test.nic_matches(MAC_ADDRESS, None).await
    })
    .await?;

    assert!(test.nic_matches(MAC_ADDRESS, None).await?);
    Ok(())
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn ovn_vm_connects_nic_with_dhcp() -> TestResult {
    let test = OvnVmControllerTest::start("ovn_vm_connects_nic_with_dhcp").await?;
    test.create_network(Some(DhcpOptions {
        cidr: DHCP_CIDR.into(),
        lease_time: None,
        dns_server: None,
        domain_name: None,
        router: None,
    }))
    .await?;
    test.create_vm().await?;

    let expected_address = format!("{MAC_ADDRESS} dynamic");
    wait_for("logical switch port with DHCP", || async {
        test.nic_matches(&expected_address, Some(DHCP_CIDR)).await
    })
    .await?;

    assert!(test.nic_matches(&expected_address, Some(DHCP_CIDR)).await?);
    Ok(())
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn ovn_vm_deletes_logical_switch_port() -> TestResult {
    let test = OvnVmControllerTest::start("ovn_vm_deletes_logical_switch_port").await?;
    test.create_network(None).await?;
    test.create_vm().await?;
    wait_for("logical switch port creation", || async {
        test.nic_matches(MAC_ADDRESS, None).await
    })
    .await?;

    test.vms().delete(VM_NAME, &DeleteParams::default()).await?;
    wait_for("logical switch port deletion", || async {
        Ok(test.port_rows().await?.is_empty())
    })
    .await?;

    assert!(test.port_rows().await?.is_empty());
    Ok(())
}
