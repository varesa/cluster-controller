use crate::cluster::controllers::virtualmachine::scheduling;
use crate::crd::network::v1beta1::{Network, NetworkSpec};
use crate::crd::virtualmachine::{
    NetworkAttachment, set_vm_status,
    v1beta3::{VirtualMachine, VirtualMachineSpec, VirtualMachineStatus},
};
use crate::integration_tests::harness::{ControlPlane, TestResult};
use crate::labels_and_annotations::{
    MAINTENANCE_ANNOTATION, MIGRATION_REQUEST_ANNOTATION, NETWORK_MODEL_LABEL,
    NO_SCHEDULE_ANNOTATION,
};
use crate::utils::traits::virtualmachine::VirtualMachineExt;
use k8s_openapi::{api::core::v1::Node, apimachinery::pkg::apis::meta::v1::ObjectMeta};
use kube::{Api, ResourceExt, api::PostParams};

fn node(name: &str) -> Node {
    Node {
        metadata: ObjectMeta {
            name: Some(name.into()),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn vm(name: &str) -> VirtualMachine {
    VirtualMachine::new(
        name,
        VirtualMachineSpec {
            cpus: 1,
            memory: "128 Mi".into(),
            volumes: vec![],
            networks: vec![],
            ..Default::default()
        },
    )
}

fn status(node: &str) -> VirtualMachineStatus {
    VirtualMachineStatus {
        scheduled: true,
        running: false,
        migration_pending: false,
        node: Some(node.into()),
        domain_name: "sample".into(),
        ip_addresses: None,
        ip_addresses_string: None,
        networks: vec![],
    }
}

fn assert_no_candidate(result: Result<Node, crate::Error>) {
    assert!(matches!(result, Err(crate::Error::ScheduleFailed(_))));
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn scheduler_avoids_maintenance_nodes() -> TestResult {
    let plane = ControlPlane::start("scheduler_avoids_maintenance_nodes").await?;
    let client = plane.client();
    let nodes: Api<Node> = Api::all(client.clone());
    let params = PostParams::default();

    let mut maintenance = node("maintenance");
    maintenance.metadata.annotations =
        Some([(MAINTENANCE_ANNOTATION.into(), "true".into())].into());
    nodes.create(&params, &maintenance).await?;
    nodes.create(&params, &node("eligible")).await?;

    let vms: Api<VirtualMachine> = Api::namespaced(client.clone(), plane.namespace());
    let vm = vms.create(&params, &vm("sample")).await?;
    let selected = scheduling::schedule(&vm, false, client.clone()).await?;
    assert_eq!(selected.name_any(), "eligible");

    // A second maintenance node leaves no candidate; randomness cannot hide a broken filter.
    let mut eligible = nodes.get("eligible").await?;
    eligible.metadata.annotations = Some([(MAINTENANCE_ANNOTATION.into(), "true".into())].into());
    nodes.replace("eligible", &params, &eligible).await?;
    assert_no_candidate(scheduling::schedule(&vm, false, client).await);
    Ok(())
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn explicit_node_bypasses_no_schedule_but_not_maintenance() -> TestResult {
    let plane =
        ControlPlane::start("explicit_node_bypasses_no_schedule_but_not_maintenance").await?;
    let client = plane.client();
    let nodes: Api<Node> = Api::all(client.clone());
    let mut requested = node("requested");
    requested.metadata.annotations = Some([(NO_SCHEDULE_ANNOTATION.into(), "true".into())].into());
    nodes.create(&PostParams::default(), &requested).await?;
    nodes
        .create(&PostParams::default(), &node("ordinary"))
        .await?;

    let vms: Api<VirtualMachine> = Api::namespaced(client.clone(), plane.namespace());
    let mut pinned = vm("pinned");
    pinned.spec.node = Some("requested".into());
    let pinned = vms.create(&PostParams::default(), &pinned).await?;
    assert_eq!(
        scheduling::schedule(&pinned, false, client.clone())
            .await?
            .name_any(),
        "requested"
    );

    let mut requested = nodes.get("requested").await?;
    requested
        .metadata
        .annotations
        .as_mut()
        .unwrap()
        .insert(MAINTENANCE_ANNOTATION.into(), "true".into());
    nodes
        .replace("requested", &PostParams::default(), &requested)
        .await?;
    assert_no_candidate(scheduling::schedule(&pinned, false, client).await);
    Ok(())
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn migration_excludes_source_and_clears_request_after_move() -> TestResult {
    let plane =
        ControlPlane::start("migration_excludes_source_and_clears_request_after_move").await?;
    let client = plane.client();
    let nodes: Api<Node> = Api::all(client.clone());
    let source = nodes
        .create(&PostParams::default(), &node("source"))
        .await?;
    nodes
        .create(&PostParams::default(), &node("destination"))
        .await?;

    let vms: Api<VirtualMachine> = Api::namespaced(client.clone(), plane.namespace());
    let vm = vms.create(&PostParams::default(), &vm("migrating")).await?;
    set_vm_status(&vm, status("source"), client.clone()).await?;
    let mut vm = vms.get("migrating").await?;
    vm.request_migration_away_from(&source, "scheduler-test", client.clone())
        .await?;
    let mut vm = vms.get("migrating").await?;
    assert!(scheduling::migration_requested(&vm));
    assert_eq!(
        scheduling::schedule(&vm, false, client.clone())
            .await?
            .name_any(),
        "destination"
    );

    scheduling::clear_successful_migration(&mut vm, client.clone(), "scheduler-test").await?;
    assert_eq!(
        vms.get("migrating")
            .await?
            .annotations()
            .get(MIGRATION_REQUEST_ANNOTATION)
            .map(String::as_str),
        Some("source")
    );

    let mut destination = nodes.get("destination").await?;
    destination.metadata.annotations =
        Some([(NO_SCHEDULE_ANNOTATION.into(), "true".into())].into());
    nodes
        .replace("destination", &PostParams::default(), &destination)
        .await?;
    assert_no_candidate(scheduling::schedule(&vm, true, client.clone()).await);

    let vm = vms.get("migrating").await?;
    set_vm_status(&vm, status("destination"), client.clone()).await?;
    let mut vm = vms.get("migrating").await?;
    assert!(!scheduling::migration_requested(&vm));
    scheduling::clear_successful_migration(&mut vm, client.clone(), "scheduler-test").await?;
    let vm = vms.get("migrating").await?;
    assert!(!vm.annotations().contains_key(MIGRATION_REQUEST_ANNOTATION));
    Ok(())
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn anti_affinity_excludes_occupied_node_unless_ignored() -> TestResult {
    let plane = ControlPlane::start("anti_affinity_excludes_occupied_node_unless_ignored").await?;
    let client = plane.client();
    let nodes: Api<Node> = Api::all(client.clone());
    nodes
        .create(&PostParams::default(), &node("occupied"))
        .await?;
    nodes.create(&PostParams::default(), &node("free")).await?;

    let vms: Api<VirtualMachine> = Api::namespaced(client.clone(), plane.namespace());
    let mut peer = vm("peer");
    peer.metadata.labels = Some([("antiAffinity".into(), "group-a".into())].into());
    let peer = vms.create(&PostParams::default(), &peer).await?;
    set_vm_status(&peer, status("occupied"), client.clone()).await?;

    let mut target = vm("target");
    target.metadata.labels = Some([("antiAffinity".into(), "group-a".into())].into());
    let target = vms.create(&PostParams::default(), &target).await?;
    assert_eq!(
        scheduling::schedule(&target, false, client.clone())
            .await?
            .name_any(),
        "free"
    );
    set_vm_status(&target, status("occupied"), client.clone()).await?;
    let target = vms.get("target").await?;
    assert!(scheduling::is_uncompliant(&target, client.clone()).await?);
    set_vm_status(&target, status("free"), client.clone()).await?;
    let target = vms.get("target").await?;
    assert!(!scheduling::is_uncompliant(&target, client.clone()).await?);
    let mut candidate = vm("candidate");
    candidate.metadata.labels = Some([("antiAffinity".into(), "group-a".into())].into());
    let candidate = vms.create(&PostParams::default(), &candidate).await?;

    let mut free = nodes.get("free").await?;
    free.metadata.annotations = Some([(NO_SCHEDULE_ANNOTATION.into(), "true".into())].into());
    nodes.replace("free", &PostParams::default(), &free).await?;
    assert_no_candidate(scheduling::schedule(&candidate, false, client.clone()).await);
    assert_eq!(
        scheduling::schedule(&candidate, true, client)
            .await?
            .name_any(),
        "occupied"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn legacy_and_single_bridge_networks_select_matching_nodes() -> TestResult {
    let plane =
        ControlPlane::start("legacy_and_single_bridge_networks_select_matching_nodes").await?;
    let client = plane.client();
    let nodes: Api<Node> = Api::all(client.clone());
    let mut legacy = node("legacy");
    legacy.metadata.labels = Some([(NETWORK_MODEL_LABEL.into(), "Legacy".into())].into());
    nodes.create(&PostParams::default(), &legacy).await?;
    let mut single = node("single");
    single.metadata.labels = Some([(NETWORK_MODEL_LABEL.into(), "SingleBridge".into())].into());
    nodes.create(&PostParams::default(), &single).await?;

    let networks: Api<Network> = Api::namespaced(client.clone(), plane.namespace());
    for (name, network_id) in [("legacy-network", None), ("single-network", Some(101))] {
        networks
            .create(
                &PostParams::default(),
                &Network::new(
                    name,
                    NetworkSpec {
                        bridge: Some("br0".into()),
                        network_id,
                        ..Default::default()
                    },
                ),
            )
            .await?;
    }

    let vms: Api<VirtualMachine> = Api::namespaced(client.clone(), plane.namespace());
    for (name, attachment, expected) in [
        (
            "direct-legacy",
            NetworkAttachment {
                bridge: Some("br0".into()),
                ..Default::default()
            },
            "legacy",
        ),
        (
            "direct-single",
            NetworkAttachment {
                bridge: Some("br0".into()),
                untagged_vlan: Some(101),
                ..Default::default()
            },
            "single",
        ),
        (
            "managed-legacy",
            NetworkAttachment {
                name: Some("legacy-network".into()),
                ..Default::default()
            },
            "legacy",
        ),
        (
            "managed-single",
            NetworkAttachment {
                name: Some("single-network".into()),
                ..Default::default()
            },
            "single",
        ),
    ] {
        let mut vm = vm(name);
        vm.spec.networks.push(attachment);
        let vm = vms.create(&PostParams::default(), &vm).await?;
        assert_eq!(
            scheduling::schedule(&vm, false, client.clone())
                .await?
                .name_any(),
            expected,
            "{name}"
        );
    }
    Ok(())
}
