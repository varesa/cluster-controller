use crate::cluster;
use crate::integration_tests::harness;
use crate::integration_tests::harness::{ControlPlane, TestResult};
use k8s_openapi::api::apps::v1::DaemonSet;
use kube::Api;

#[tokio::test]
#[ignore = "requires control plane"]
async fn create_host_daemonset() -> TestResult {
    let plane = ControlPlane::start("create_host_daemonset").await?;
    let client = plane.client();

    cluster::run_daemonset(client.clone(), plane.namespace()).await?;
    let daemonsets: Api<DaemonSet> = Api::namespaced(client.clone(), plane.namespace());
    let first = daemonsets.get("libvirt-host-controller").await?;

    // Applying the same daemonset again must not replace the workload or conflict
    // with the field ownership of its first server-side apply.
    cluster::run_daemonset(client, plane.namespace()).await?;
    let observed = daemonsets.get("libvirt-host-controller").await?;
    assert_eq!(observed.metadata.uid, first.metadata.uid);
    assert_eq!(
        observed.metadata.namespace.as_deref(),
        Some(plane.namespace())
    );
    let ds = observed.spec.expect("host daemonset spec");
    let selector = ds.selector.match_labels.expect("host daemonset selector");
    assert_eq!(
        selector.get("name").map(String::as_str),
        Some("libvirt-host-controller")
    );
    let labels = ds
        .template
        .metadata
        .expect("host pod metadata")
        .labels
        .expect("host pod labels");
    assert_eq!(
        labels.get("name").map(String::as_str),
        Some("libvirt-host-controller")
    );
    let pod = ds.template.spec.expect("host pod spec");
    assert_eq!(pod.host_network, Some(true));
    assert!(
        pod.node_selector.is_none(),
        "host daemonset must run on every node"
    );
    assert!(
        pod.affinity.is_none(),
        "host daemonset must not restrict eligible nodes"
    );
    let volumes = pod.volumes.expect("host volumes");
    for (name, path) in [
        ("virtqemud-sock", "/var/run/libvirt/virtqemud-sock"),
        ("ceph-config", "/etc/ceph"),
    ] {
        assert!(
            volumes.iter().any(|volume| volume.name == name
                && volume
                    .host_path
                    .as_ref()
                    .is_some_and(|host| host.path == path)),
            "missing host path {name}: {path}"
        );
    }
    assert_eq!(pod.containers.len(), 1);
    let container = &pod.containers[0];
    assert_eq!(container.name, "libvirt-host-controller");
    assert_eq!(container.image.as_deref(), Some(harness::IMAGE));
    assert_eq!(
        container.command.as_deref(),
        Some(&["cluster-controller".to_owned(), "--host".to_owned()][..])
    );
    assert_eq!(
        container
            .security_context
            .as_ref()
            .and_then(|context| context.privileged),
        Some(true)
    );
    let mounts = container
        .volume_mounts
        .as_ref()
        .expect("host volume mounts");
    for (name, path) in [
        ("virtqemud-sock", "/var/run/libvirt/virtqemud-sock"),
        ("ceph-config", "/etc/ceph"),
    ] {
        assert!(
            mounts
                .iter()
                .any(|mount| mount.name == name && mount.mount_path == path),
            "missing host mount {name}: {path}"
        );
    }
    assert!(
        container
            .env
            .as_ref()
            .is_some_and(|env| env.iter().any(|entry| entry.name == "NODE_NAME"
                && entry
                    .value_from
                    .as_ref()
                    .and_then(|source| source.field_ref.as_ref())
                    .is_some_and(|field| field.field_path == "spec.nodeName")))
    );
    Ok(())
}
