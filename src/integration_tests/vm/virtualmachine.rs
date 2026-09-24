use std::time::Duration;

use k8s_openapi::{api::core::v1::Node, apimachinery::pkg::apis::meta::v1::ObjectMeta};
use kube::{Api, Client, api::PostParams};
use tokio::{
    task::JoinHandle,
    time::{sleep, timeout},
};
use uuid::Uuid;

use crate::cluster::controllers::virtualmachine::{
    utils::{fill_nics, fill_uuid},
    vm,
};
use crate::crd::{
    network::{
        NetworkType,
        v1beta1::{Network, NetworkSpec},
    },
    virtualmachine::{
        NetworkAttachment, VirtualMachineStatus, set_vm_status,
        v1beta3::{VirtualMachine, VirtualMachineSpec},
    },
};
use crate::integration_tests::harness::{ControlPlane, TestResult};
use crate::labels_and_annotations::MIGRATION_REQUEST_ANNOTATION;

const RECONCILE_TIMEOUT: Duration = Duration::from_secs(90);
const POLL_INTERVAL: Duration = Duration::from_millis(100);

struct ControllerTask(JoinHandle<Result<(), crate::Error>>);

impl ControllerTask {
    fn start(client: Client) -> Self {
        Self(tokio::spawn(vm::create(client)))
    }
}

impl Drop for ControllerTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn vm_spec(networks: Vec<NetworkAttachment>) -> VirtualMachineSpec {
    VirtualMachineSpec {
        cpus: 1,
        memory: "128 Mi".into(),
        networks,
        ..Default::default()
    }
}

async fn node(client: Client, name: &str) -> TestResult {
    let nodes: Api<Node> = Api::all(client);
    nodes
        .create(
            &PostParams::default(),
            &Node {
                metadata: ObjectMeta {
                    name: Some(name.into()),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .await?;
    Ok(())
}

async fn wait_for_vm(
    vms: &Api<VirtualMachine>,
    name: &str,
    ready: impl Fn(&VirtualMachine) -> bool,
) -> TestResult<VirtualMachine> {
    let result = timeout(RECONCILE_TIMEOUT, async {
        loop {
            let vm = vms.get(name).await?;
            if ready(&vm) {
                return Ok::<_, kube::Error>(vm);
            }
            sleep(POLL_INTERVAL).await;
        }
    })
    .await
    .map_err(|_| format!("timed out waiting for {name} reconciliation"))??;
    Ok(result)
}

// A status-subresource write changes resourceVersion. fill_nics also commits the
// VM spec, so on a changed NIC its first attempt can conflict after persisting
// status. Re-read the object before retrying, as the controller's watch does.
async fn fill_nics_from_api(vms: &Api<VirtualMachine>, client: Client) -> TestResult {
    for _ in 0..3 {
        let mut current = vms.get("sample").await?;
        match fill_nics(&mut current, client.clone()).await {
            Ok(()) => return Ok(()),
            Err(crate::Error::Kube(kube::Error::Api(response))) if response.code == 409 => {}
            Err(error) => return Err(error.into()),
        }
    }
    Err("fill_nics did not commit after retrying with current resourceVersion".into())
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn controller_initializes_schedules_and_preserves_identity() -> TestResult {
    let plane = ControlPlane::start("vm_controller_initializes_schedules").await?;
    let client = plane.client();
    node(client.clone(), "eligible").await?;
    let networks: Api<Network> = Api::namespaced(client.clone(), plane.namespace());
    networks
        .create(
            &PostParams::default(),
            &Network::new("ovn-net", NetworkSpec::default()),
        )
        .await?;
    let vms: Api<VirtualMachine> = Api::namespaced(client.clone(), plane.namespace());
    vms.create(
        &PostParams::default(),
        &VirtualMachine::new(
            "sample",
            vm_spec(vec![NetworkAttachment {
                name: Some("ovn-net".into()),
                queues: Some(1),
                ..Default::default()
            }]),
        ),
    )
    .await?;
    let _controller = ControllerTask::start(client);

    let first = wait_for_vm(&vms, "sample", |vm| {
        vm.status.as_ref().is_some_and(|status| {
            status.scheduled
                && status.node.as_deref() == Some("eligible")
                && status
                    .networks
                    .first()
                    .and_then(|nic| nic.ovn_id.as_ref())
                    .is_some()
        }) && vm.spec.uuid.is_some()
    })
    .await?;
    let status = first.status.as_ref().unwrap();
    assert_eq!(status.domain_name, format!("{}-sample", plane.namespace()));
    assert!(!status.running);
    assert!(!status.migration_pending);
    assert_eq!(status.networks.len(), 1);
    let original_nic = status.networks[0].clone();
    let original_uuid = first.spec.uuid.clone().unwrap();
    Uuid::parse_str(&original_uuid)?;
    Uuid::parse_str(original_nic.ovn_id.as_deref().unwrap())?;
    assert!(
        original_nic
            .mac_address
            .as_deref()
            .is_some_and(|mac| mac.starts_with("52:54:00:"))
    );
    assert_eq!(first.spec.networks[0].ovn_id, None);
    assert_eq!(first.spec.networks[0].mac_address, None);

    // Replace the freshly fetched object with its resourceVersion, exercising a
    // second reconciliation without removing the controller-generated identity.
    let mut updated = vms.get("sample").await?;
    updated.spec.networks[0].queues = Some(2);
    vms.replace("sample", &PostParams::default(), &updated)
        .await?;
    let second = wait_for_vm(&vms, "sample", |vm| {
        vm.status.as_ref().is_some_and(|status| {
            status
                .networks
                .first()
                .is_some_and(|nic| nic.queues == Some(2))
        })
    })
    .await?;
    let second_status = second.status.unwrap();
    assert_eq!(second.spec.uuid.as_deref(), Some(original_uuid.as_str()));
    assert_eq!(second_status.node.as_deref(), Some("eligible"));
    assert_eq!(
        second_status.networks[0].mac_address,
        original_nic.mac_address
    );
    assert_eq!(second_status.networks[0].ovn_id, original_nic.ovn_id);
    Ok(())
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn nic_modes_and_uuid_are_stable_on_direct_reconciliation() -> TestResult {
    let plane = ControlPlane::start("vm_nic_modes_and_uuid").await?;
    let client = plane.client();
    let networks: Api<Network> = Api::namespaced(client.clone(), plane.namespace());
    networks
        .create(
            &PostParams::default(),
            &Network::new(
                "evpn-net",
                NetworkSpec {
                    network_type: Some(NetworkType::Evpn),
                    network_id: Some(208),
                    bridge: Some("br-evpn".into()),
                    ..Default::default()
                },
            ),
        )
        .await?;
    let vms: Api<VirtualMachine> = Api::namespaced(client.clone(), plane.namespace());
    let vm = vms
        .create(
            &PostParams::default(),
            &VirtualMachine::new(
                "sample",
                vm_spec(vec![
                    NetworkAttachment {
                        name: Some("evpn-net".into()),
                        queues: Some(2),
                        ..Default::default()
                    },
                    NetworkAttachment {
                        bridge: Some("br-local".into()),
                        mac_address: Some("52:54:00:12:34:56".into()),
                        ..Default::default()
                    },
                ]),
            ),
        )
        .await?;
    set_vm_status(
        &vm,
        VirtualMachineStatus {
            scheduled: false,
            running: false,
            migration_pending: false,
            node: None,
            domain_name: format!("{}-sample", plane.namespace()),
            ip_addresses: None,
            ip_addresses_string: None,
            networks: vec![],
        },
        client.clone(),
    )
    .await?;

    fill_nics_from_api(&vms, client.clone()).await?;
    let first = vms.get("sample").await?;
    let status = first.status.as_ref().unwrap();
    assert_eq!(status.networks.len(), 2);
    let evpn = &status.networks[0];
    assert_eq!(evpn.name.as_deref(), Some("evpn-net"));
    assert_eq!(evpn.bridge.as_deref(), Some("br-evpn"));
    assert_eq!(evpn.untagged_vlan, Some(208));
    assert_eq!(evpn.queues, Some(2));
    assert_eq!(evpn.ovn_id, None);
    assert!(
        evpn.mac_address
            .as_deref()
            .is_some_and(|mac| mac.starts_with("52:54:00:"))
    );
    let local = &status.networks[1];
    assert_eq!(local.bridge.as_deref(), Some("br-local"));
    assert_eq!(local.mac_address.as_deref(), Some("52:54:00:12:34:56"));
    assert_eq!(local.ovn_id, None);

    let mut current = vms.get("sample").await?;
    fill_uuid(&mut current, client.clone()).await?;
    let generated = vms.get("sample").await?;
    let uuid = generated.spec.uuid.as_deref().unwrap();
    Uuid::parse_str(uuid)?;
    let original_uuid = uuid.to_owned();
    let original_networks = generated.status.as_ref().unwrap().networks.clone();
    let mut current = generated;
    fill_nics(&mut current, client.clone()).await?;
    let mut current = vms.get("sample").await?;
    fill_uuid(&mut current, client.clone()).await?;
    let stable = vms.get("sample").await?;
    assert_eq!(stable.spec.uuid.as_deref(), Some(original_uuid.as_str()));
    assert_eq!(stable.status.as_ref().unwrap().networks, original_networks);

    // Removing a NIC from spec must remove it from status, while retaining the
    // surviving interface's generated MAC and EVPN VLAN information.
    let mut updated = stable;
    updated.spec.networks.remove(1);
    vms.replace("sample", &PostParams::default(), &updated)
        .await?;
    fill_nics_from_api(&vms, client).await?;
    let pruned = vms.get("sample").await?;
    assert_eq!(pruned.status.unwrap().networks, original_networks[..1]);
    assert_eq!(pruned.spec.uuid.as_deref(), Some(original_uuid.as_str()));
    Ok(())
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn controller_marks_requested_migration_pending() -> TestResult {
    let plane = ControlPlane::start("vm_controller_migration").await?;
    let client = plane.client();
    node(client.clone(), "source").await?;
    let vms: Api<VirtualMachine> = Api::namespaced(client.clone(), plane.namespace());
    vms.create(
        &PostParams::default(),
        &VirtualMachine::new("sample", vm_spec(vec![])),
    )
    .await?;
    let _controller = ControllerTask::start(client.clone());
    let before = wait_for_vm(&vms, "sample", |vm| {
        vm.status
            .as_ref()
            .is_some_and(|status| status.scheduled && status.node.as_deref() == Some("source"))
            && vm.spec.uuid.is_some()
    })
    .await?;
    assert!(!before.status.as_ref().unwrap().migration_pending);

    node(client, "target").await?;
    let mut requested = vms.get("sample").await?;
    requested
        .metadata
        .annotations
        .get_or_insert_with(Default::default)
        .insert(MIGRATION_REQUEST_ANNOTATION.into(), "source".into());
    vms.replace("sample", &PostParams::default(), &requested)
        .await?;
    let after = wait_for_vm(&vms, "sample", |vm| {
        vm.status.as_ref().is_some_and(|status| {
            status.node.as_deref() == Some("target") && status.migration_pending
        }) && vm
            .metadata
            .annotations
            .as_ref()
            .is_none_or(|annotations| !annotations.contains_key(MIGRATION_REQUEST_ANNOTATION))
    })
    .await?;
    assert!(after.status.as_ref().unwrap().scheduled);
    assert!(!after.status.as_ref().unwrap().running);
    assert_eq!(after.spec.uuid, before.spec.uuid);
    Ok(())
}
