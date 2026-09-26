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
    cluster::controllers::virtualmachine::ovn,
    crd::{
        network::v1beta1::{Network, NetworkSpec},
        virtualmachine::{
            NetworkAttachment,
            v1beta3::{VirtualMachine, VirtualMachineSpec, VirtualMachineStatus},
        },
    },
    errors::Error,
    interfaces::ovn::{
        common::{OvnBasicActions, OvnCommon, OvnNamedGetters},
        lowlevel::Ovn,
        types::{logicalswitch::LogicalSwitch, logicalswitchport::LogicalSwitchPort},
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
async fn ovn_vm_connects_nic_updates_status_and_disconnects_before_deletion() -> TestResult {
    let plane =
        ControlPlane::start("ovn_vm_connects_nic_updates_status_and_disconnects_before_deletion")
            .await?;
    let _backend = OvnNorthbound::start(plane.client()).await?;
    let ovn_db = Arc::new(Ovn::try_new("127.0.0.1", 6641)?);
    let switch_name = format!("{}-tenant", plane.namespace());
    LogicalSwitch::create(ovn_db.clone(), &switch_name)?;

    let client = plane.client();
    let networks: Api<Network> = Api::namespaced(client.clone(), plane.namespace());
    networks
        .create(
            &PostParams::default(),
            &Network::new("tenant", NetworkSpec::default()),
        )
        .await?;

    let nic = NetworkAttachment {
        name: Some("tenant".into()),
        mac_address: Some("02:00:00:00:00:42".into()),
        ovn_id: Some("test-vm-nic".into()),
        ..Default::default()
    };
    let vms: Api<VirtualMachine> = Api::namespaced(client.clone(), plane.namespace());
    vms.create(
        &PostParams::default(),
        &VirtualMachine::new(
            "test-vm",
            VirtualMachineSpec {
                cpus: 1,
                memory: "128 Mi".into(),
                networks: vec![nic.clone()],
                ..Default::default()
            },
        ),
    )
    .await?;
    vms.patch_status(
        "test-vm",
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

    let _controller = AbortOnDrop(tokio::spawn(ovn::create(client)));
    timeout(Duration::from_secs(30), async {
        loop {
            let vm = vms.get("test-vm").await?;
            let port = LogicalSwitchPort::get_by_name(ovn_db.clone(), "test-vm-nic");
            let connected = match port {
                Ok(port) => LogicalSwitch::get_by_name(ovn_db.clone(), &switch_name)?
                    .port_ids()
                    .contains(&port.uuid()),
                Err(Error::OvnNotFound(_, _)) => false,
                Err(error) => return Err(error.into()),
            };
            if connected
                && vm.metadata.finalizers.as_ref().is_some_and(|finalizers| {
                    finalizers
                        .iter()
                        .any(|name| name == "cluster-virt.acl.fi/ovn")
                })
                && vm.status.as_ref().is_some_and(|status| {
                    status.ip_addresses.as_ref().is_some_and(Vec::is_empty)
                        && status.ip_addresses_string.as_deref() == Some("")
                })
            {
                break Ok::<(), Box<dyn std::error::Error + Send + Sync>>(());
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await??;

    vms.delete("test-vm", &DeleteParams::default()).await?;
    timeout(Duration::from_secs(30), async {
        loop {
            if vms.get_opt("test-vm").await?.is_none()
                && matches!(
                    LogicalSwitchPort::get_by_name(ovn_db.clone(), "test-vm-nic"),
                    Err(Error::OvnNotFound(_, _))
                )
                && LogicalSwitch::get_by_name(ovn_db.clone(), &switch_name)?
                    .port_ids()
                    .is_empty()
            {
                break Ok::<(), Box<dyn std::error::Error + Send + Sync>>(());
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await??;
    Ok(())
}
