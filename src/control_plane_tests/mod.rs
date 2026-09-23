//! GitLab-only samples using an isolated Kubernetes API, without controller bootstrap or daemons.
//! Finalizer handling here demonstrates API semantics, not hypervisor cleanup.

mod harness;

use std::time::Duration;

use k8s_openapi::{api::core::v1::Node, apimachinery::pkg::apis::meta::v1::ObjectMeta};
use kube::{
    Api,
    api::{DeleteParams, PostParams},
};
use tokio::time::{sleep, timeout};

use crate::cluster::controllers::virtualmachine::scheduling;
use crate::crd::virtualmachine::v1beta3::{VirtualMachine, VirtualMachineSpec};
use crate::labels_and_annotations::MAINTENANCE_ANNOTATION;
use harness::{ControlPlane, TestResult};

#[tokio::test]
#[cfg(feature = "control-plane-tests")]
async fn scheduler_avoids_maintenance_nodes() -> TestResult {
    let plane = ControlPlane::start("scheduler_avoids_maintenance_nodes").await?;
    let client = plane.client();
    let nodes: Api<Node> = Api::all(client.clone());
    let params = PostParams::default();

    for (name, maintenance) in [("maintenance", true), ("eligible", false)] {
        let node = Node {
            metadata: ObjectMeta {
                name: Some(name.into()),
                annotations: maintenance
                    .then(|| [(MAINTENANCE_ANNOTATION.into(), "true".into())].into()),
                ..Default::default()
            },
            ..Default::default()
        };
        nodes.create(&params, &node).await?;
    }

    let vms: Api<VirtualMachine> = Api::namespaced(client.clone(), plane.namespace());
    let vm = VirtualMachine::new(
        "sample",
        VirtualMachineSpec {
            cpus: 1,
            memory: "128 Mi".into(),
            volumes: vec![],
            networks: vec![],
            ..Default::default()
        },
    );
    vms.create(&params, &vm).await?;
    let vm = vms.get("sample").await?;

    let selected = scheduling::schedule(&vm, false, client.clone()).await?;
    assert_eq!(selected.metadata.name.as_deref(), Some("eligible"));

    // With every node in maintenance, a missing filter must fail deterministically,
    // rather than occasionally passing because the random choice picked "eligible".
    let mut eligible = nodes.get("eligible").await?;
    eligible.metadata.annotations = Some([(MAINTENANCE_ANNOTATION.into(), "true".into())].into());
    nodes.replace("eligible", &params, &eligible).await?;
    assert!(matches!(
        scheduling::schedule(&vm, false, client).await,
        Err(crate::Error::ScheduleFailed(_))
    ));
    Ok(())
}

#[tokio::test]
#[cfg(feature = "control-plane-tests")]
async fn vm_deletion_waits_for_finalizer() -> TestResult {
    let plane = ControlPlane::start("vm_deletion_waits_for_finalizer").await?;
    let vms: Api<VirtualMachine> = Api::namespaced(plane.client(), plane.namespace());
    let mut vm = VirtualMachine::new(
        "sample",
        VirtualMachineSpec {
            cpus: 1,
            memory: "128 Mi".into(),
            volumes: vec![],
            networks: vec![],
            ..Default::default()
        },
    );
    vm.metadata.finalizers = Some(vec!["test.cluster-virt.acl.fi/hold-deletion".into()]);
    vms.create(&PostParams::default(), &vm).await?;

    vms.delete("sample", &DeleteParams::default()).await?;
    let mut deleting = vms.get("sample").await?;
    assert!(deleting.metadata.deletion_timestamp.is_some());

    // Preserve the GET's resourceVersion: concurrent changes must conflict, not be overwritten.
    deleting.metadata.finalizers = None;
    vms.replace("sample", &PostParams::default(), &deleting)
        .await?;
    timeout(Duration::from_secs(10), async {
        while vms.get_opt("sample").await?.is_some() {
            sleep(Duration::from_millis(100)).await;
        }
        Ok::<(), kube::Error>(())
    })
    .await??;
    Ok(())
}
