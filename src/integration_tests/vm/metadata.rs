use crate::integration_tests::harness;
use crate::integration_tests::harness::{ControlPlane, TestResult};
use crate::metadataservice::deployment;
use k8s_openapi::{
    api::{
        apps::v1::Deployment,
        core::v1::{Namespace, ServiceAccount},
        rbac::v1::{ClusterRole, ClusterRoleBinding},
    },
    apimachinery::pkg::apis::meta::v1::ObjectMeta,
};
use kube::{Api, Client, api::PostParams};
use std::time::Duration;
use tokio::time::{sleep, timeout};

const CONTROLLER: &str = "metadata-test-controller";
const ROUTER: &str = "test-router";
const TEST_NAMESPACE: &str = "mds-test";

async fn create_test_namespace(client: Client) -> TestResult {
    let namespaces: Api<Namespace> = Api::all(client);
    namespaces
        .create(
            &PostParams::default(),
            &Namespace {
                metadata: ObjectMeta {
                    name: Some(TEST_NAMESPACE.into()),
                    ..ObjectMeta::default()
                },
                ..Namespace::default()
            },
        )
        .await?;
    Ok(())
}

async fn assert_rbac(client: Client, namespace: &str) -> TestResult {
    let service_accounts: Api<ServiceAccount> = Api::namespaced(client.clone(), namespace);
    let account = service_accounts.get("metadata-service").await?;
    assert_eq!(account.metadata.namespace.as_deref(), Some(namespace));

    let roles: Api<ClusterRole> = Api::all(client.clone());
    let role = roles.get("metadata-service").await?;
    let rules = role.rules.expect("metadata role has rules");
    assert_eq!(rules.len(), 3);
    for (group, resource, verb) in [
        ("cluster-virt.acl.fi", "virtualmachines", "list"),
        ("", "configmaps", "get"),
        ("", "nodes", "list"),
    ] {
        assert!(
            rules.iter().any(|rule| {
                rule.api_groups
                    .as_deref()
                    .is_some_and(|groups| groups.len() == 1 && groups[0] == group)
                    && rule
                        .resources
                        .as_deref()
                        .is_some_and(|resources| resources.len() == 1 && resources[0] == resource)
                    && rule.verbs.len() == 1
                    && rule.verbs[0] == verb
            }),
            "missing metadata role rule: {group}/{resource}/{verb}"
        );
    }

    let bindings: Api<ClusterRoleBinding> = Api::all(client);
    let binding = bindings
        .get(&format!("metadata-service-{namespace}"))
        .await?;
    assert_eq!(binding.role_ref.api_group, "rbac.authorization.k8s.io");
    assert_eq!(binding.role_ref.kind, "ClusterRole");
    assert_eq!(binding.role_ref.name, "metadata-service");
    let subjects = binding.subjects.expect("metadata binding has subjects");
    assert_eq!(subjects.len(), 1);
    assert_eq!(subjects[0].kind, "ServiceAccount");
    assert_eq!(subjects[0].name, "metadata-service");
    assert_eq!(subjects[0].namespace.as_deref(), Some(namespace));
    Ok(())
}

async fn assert_deployment(client: Client, namespace: &str) -> TestResult {
    let deployments: Api<Deployment> = Api::namespaced(client, namespace);
    let name = format!("metadata-{namespace}-{ROUTER}");
    let applied = timeout(Duration::from_secs(10), async {
        loop {
            if let Some(applied) = deployments.get_opt(&name).await? {
                return Ok::<Deployment, kube::Error>(applied);
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await??;
    assert_eq!(applied.metadata.namespace.as_deref(), Some(namespace));
    let spec = applied.spec.expect("metadata deployment has a spec");
    let pod = spec.template.spec.expect("metadata deployment has a pod");
    assert_eq!(
        pod.service_account_name.as_deref(),
        Some("metadata-service")
    );
    assert_eq!(pod.host_network, Some(true));
    assert_eq!(pod.containers.len(), 1);
    assert_eq!(pod.containers[0].image.as_deref(), Some(harness::IMAGE));
    assert_eq!(
        pod.containers[0].command.as_deref(),
        Some(
            &[
                "cluster-controller".to_string(),
                "--metadata-service".to_string(),
                format!("{namespace}/{ROUTER}"),
            ][..]
        )
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn metadata_create_rbac_applies_service_account_and_permissions() -> TestResult {
    let plane = ControlPlane::start("metadata_create_rbac").await?;
    create_test_namespace(plane.client()).await?;
    deployment::create_rbac(plane.client(), TEST_NAMESPACE, CONTROLLER).await?;
    assert_rbac(plane.client(), TEST_NAMESPACE).await
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn metadata_create_deployment_uses_running_controller_image() -> TestResult {
    let plane = ControlPlane::start("metadata_create_deployment").await?;
    create_test_namespace(plane.client()).await?;
    deployment::create_deployment(plane.client(), CONTROLLER, TEST_NAMESPACE, ROUTER).await?;
    assert_deployment(plane.client(), TEST_NAMESPACE).await
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn metadata_deploy_applies_rbac_and_workload() -> TestResult {
    let plane = ControlPlane::start("metadata_deploy").await?;
    create_test_namespace(plane.client()).await?;
    deployment::deploy(plane.client(), CONTROLLER, TEST_NAMESPACE, ROUTER).await?;
    assert_rbac(plane.client(), TEST_NAMESPACE).await?;
    assert_deployment(plane.client(), TEST_NAMESPACE).await
}
