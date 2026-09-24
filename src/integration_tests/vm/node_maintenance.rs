use std::time::Duration;

use k8s_openapi::{api::core::v1::Node, apimachinery::pkg::apis::meta::v1::ObjectMeta};
use kube::{
    Api,
    api::{Patch, PatchParams, PostParams},
};
use serde_json::json;
use tokio::{
    task::JoinHandle,
    time::{sleep, timeout},
};

use crate::cluster::controllers::node;
use crate::crd::virtualmachine::v1beta3::{VirtualMachine, VirtualMachineSpec};
use crate::integration_tests::harness::{ControlPlane, TestResult};
use crate::labels_and_annotations::{MAINTENANCE_ANNOTATION, MIGRATION_REQUEST_ANNOTATION};

struct AbortOnDrop(JoinHandle<Result<(), crate::Error>>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn migration_request(vms: &Api<VirtualMachine>, name: &str) -> TestResult<Option<String>> {
    Ok(vms
        .get(name)
        .await?
        .metadata
        .annotations
        .as_ref()
        .and_then(|annotations| annotations.get(MIGRATION_REQUEST_ANNOTATION))
        .cloned())
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn node_maintenance_requests_migration_only_for_its_vms() -> TestResult {
    let plane = ControlPlane::start("node_maintenance_requests_migration_only_for_its_vms").await?;
    let client = plane.client();
    let nodes: Api<Node> = Api::all(client.clone());
    let vms: Api<VirtualMachine> = Api::namespaced(client.clone(), plane.namespace());

    for name in ["host-a", "host-b"] {
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
    }
    for (name, node) in [("on-a", "host-a"), ("on-b", "host-b")] {
        let vm = VirtualMachine::new(
            name,
            VirtualMachineSpec {
                cpus: 1,
                memory: "128 Mi".into(),
                volumes: vec![],
                networks: vec![],
                ..Default::default()
            },
        );
        vms.create(&PostParams::default(), &vm).await?;
        vms.patch_status(
            name,
            &PatchParams::default(),
            &Patch::Merge(json!({"status": {
                "scheduled": true,
                "running": false,
                "migration_pending": false,
                "node": node,
                "domain_name": name,
                "networks": []
            }})),
        )
        .await?;
        assert_eq!(migration_request(&vms, name).await?, None);
    }

    // Nodes already exist when the controller begins watching: this exercises the
    // initial watch and then subsequent node annotation updates.
    let _watcher = AbortOnDrop(tokio::spawn(node::create(client)));
    nodes
        .patch(
            "host-a",
            &PatchParams::default(),
            &Patch::Merge(json!({"metadata": {"annotations": {(MAINTENANCE_ANNOTATION): "true"}}})),
        )
        .await?;

    timeout(Duration::from_secs(15), async {
        loop {
            if migration_request(&vms, "on-a").await?.as_deref() == Some("host-a") {
                break Ok::<(), Box<dyn std::error::Error + Send + Sync>>(());
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await??;
    // The controller lists all VMs; let that reconcile finish before declaring
    // the VM on the non-maintenance host unaffected.
    sleep(Duration::from_millis(500)).await;
    assert_eq!(migration_request(&vms, "on-b").await?, None);

    // Switching maintenance hosts must target the other VM only.
    nodes
        .patch(
            "host-a",
            &PatchParams::default(),
            &Patch::Merge(
                json!({"metadata": {"annotations": {(MAINTENANCE_ANNOTATION): "false"}}}),
            ),
        )
        .await?;
    nodes
        .patch(
            "host-b",
            &PatchParams::default(),
            &Patch::Merge(json!({"metadata": {"annotations": {(MAINTENANCE_ANNOTATION): "true"}}})),
        )
        .await?;
    timeout(Duration::from_secs(15), async {
        loop {
            if migration_request(&vms, "on-b").await?.as_deref() == Some("host-b") {
                break Ok::<(), Box<dyn std::error::Error + Send + Sync>>(());
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await??;
    // Give any remaining reconcile in the watch batch time to run before
    // asserting that the unrelated VM remains untouched.
    sleep(Duration::from_millis(500)).await;
    assert_eq!(
        migration_request(&vms, "on-a").await?.as_deref(),
        Some("host-a")
    );
    assert_eq!(
        migration_request(&vms, "on-b").await?.as_deref(),
        Some("host-b")
    );
    Ok(())
}
