use crate::cluster::controllers::ovn_services::{ovn_central, ovn_controller};
use crate::integration_tests::harness::{ControlPlane, TestResult};
use crate::labels_and_annotations::{OVN_CENTRAL_MANAGED_LABEL, OVN_CONTROLLER_MANAGEMENT_LABEL};
use k8s_openapi::api::{
    apps::v1::DaemonSet,
    core::v1::{Container, PodSpec},
};
use kube::Api;
use std::time::Duration;
use tokio::{
    task::JoinHandle,
    time::{interval, timeout},
};

// Both create functions remain in an idle loop after applying their DaemonSet.
// Aborting on every exit path keeps them from using the stopped test API server.
struct OvnTasks {
    controller: JoinHandle<Result<(), crate::errors::Error>>,
    central: JoinHandle<Result<(), crate::errors::Error>>,
}

impl Drop for OvnTasks {
    fn drop(&mut self) {
        self.controller.abort();
        self.central.abort();
    }
}

fn assert_pod<'a>(daemonset: &'a DaemonSet, name: &str, node_label: &str) -> &'a PodSpec {
    let spec = daemonset.spec.as_ref().expect("DaemonSet spec");
    assert_eq!(
        spec.selector
            .match_labels
            .as_ref()
            .and_then(|labels| labels.get("app"))
            .map(String::as_str),
        Some(name),
    );
    assert_eq!(
        spec.template
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.labels.as_ref())
            .and_then(|labels| labels.get("app"))
            .map(String::as_str),
        Some(name),
    );
    let pod = spec.template.spec.as_ref().expect("pod spec");
    assert_eq!(
        pod.node_selector
            .as_ref()
            .and_then(|labels| labels.get(node_label))
            .map(String::as_str),
        Some("managed"),
    );
    assert_eq!(pod.host_network, Some(true));
    pod
}

fn assert_privileged_mounts(container: &Container, mounts: &[(&str, &str)]) {
    assert_eq!(
        container
            .security_context
            .as_ref()
            .and_then(|security| security.privileged),
        Some(true),
        "{} must run privileged",
        container.name,
    );
    let actual = container.volume_mounts.as_ref().expect("volume mounts");
    assert_eq!(
        actual.len(),
        mounts.len(),
        "{} volume mounts",
        container.name
    );
    for (name, path) in mounts {
        assert!(
            actual
                .iter()
                .any(|mount| mount.name == *name && mount.mount_path == *path),
            "{} missing mount {name} at {path}",
            container.name,
        );
    }
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn ovn_services_create_daemonsets() -> TestResult {
    let plane = ControlPlane::start("ovn_services_create_daemonsets").await?;
    let client = plane.client();

    let mut tasks = OvnTasks {
        controller: tokio::spawn(ovn_controller::create(client.clone())),
        central: tokio::spawn(ovn_central::create(client.clone())),
    };
    let daemonsets: Api<DaemonSet> = Api::namespaced(client, crate::NAMESPACE);
    let (controller, central) = timeout(Duration::from_secs(15), async {
        let mut ticks = interval(Duration::from_millis(100));
        loop {
            tokio::select! {
                result = &mut tasks.controller => {
                    return Err(format!("OVN controller exited before reconciliation: {result:?}").into());
                }
                result = &mut tasks.central => {
                    return Err(format!("OVN central exited before reconciliation: {result:?}").into());
                }
                _ = ticks.tick() => {}
            }
            if let (Some(controller), Some(central)) = (
                daemonsets.get_opt("ovn-controller").await?,
                daemonsets.get_opt("ovn-central").await?,
            ) {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>((controller, central));
            }
        }
    }).await.map_err(|_| "timed out waiting for both OVN DaemonSets")??;

    let pod = assert_pod(
        &controller,
        "ovn-controller",
        OVN_CONTROLLER_MANAGEMENT_LABEL,
    );
    assert_eq!(pod.containers.len(), 1);
    let container = &pod.containers[0];
    assert_eq!(container.name, "ovn-controller");
    assert_eq!(
        container
            .command
            .as_deref()
            .and_then(|command| command.first())
            .map(String::as_str),
        Some("ovn-controller"),
    );
    assert_privileged_mounts(
        container,
        &[
            ("ovs-run", "/var/run/openvswitch"),
            ("ovn-run", "/var/run/ovn"),
        ],
    );
    let volumes = pod.volumes.as_ref().expect("OVN controller volumes");
    assert_eq!(volumes.len(), 2);
    let ovs = volumes
        .iter()
        .find(|volume| volume.name == "ovs-run")
        .expect("OVS host volume");
    let host_path = ovs.host_path.as_ref().expect("OVS host path");
    assert_eq!(host_path.path, "/var/run/openvswitch");
    assert_eq!(host_path.type_.as_deref(), Some("Directory"));
    let ovn = volumes
        .iter()
        .find(|volume| volume.name == "ovn-run")
        .expect("OVN runtime volume");
    assert!(
        ovn.empty_dir.is_some(),
        "OVN runtime directory must be ephemeral"
    );

    let pod = assert_pod(&central, "ovn-central", OVN_CENTRAL_MANAGED_LABEL);
    let mounts = [
        ("var-run-openvswitch", "/var/run/openvswitch"),
        ("var-run-ovn", "/var/run/ovn"),
        ("var-lib-ovn", "/var/lib/ovn"),
        ("etc-openvswitch", "/etc/openvswitch"),
    ];
    assert_eq!(pod.containers.len(), 3);
    for (container, name) in pod.containers.iter().zip(["nbdb", "sbdb", "northd"]) {
        assert_eq!(container.name, name);
        assert_privileged_mounts(container, &mounts);
    }
    let volumes = pod.volumes.as_ref().expect("OVN central volumes");
    assert_eq!(volumes.len(), mounts.len());
    for (name, path) in mounts {
        let volume = volumes
            .iter()
            .find(|volume| volume.name == name)
            .expect("central host volume");
        let host_path = volume.host_path.as_ref().expect("central host path");
        assert_eq!(host_path.path, path);
        assert_eq!(host_path.type_.as_deref(), Some("DirectoryOrCreate"));
    }

    Ok(())
}
