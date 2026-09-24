use crate::Error;
use crate::crd::cluster::{Cluster, ClusterSpec};
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::Namespace;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::PostParams;
use kube::{Api, Client};
use serde_json::json;

pub(super) async fn cluster_resources(
    client: &Client,
    namespace: &str,
    image: &str,
) -> Result<(), Error> {
    // Seed the Cluster CustomResource
    create_cluster_cr(client).await?;

    // Create a mock cluster controller deployment so that
    // controllers that attempt to get the running image have something to read
    create_namespace(client, namespace).await?;
    create_controller_deployment(client, namespace, image).await?;

    Ok(())
}

async fn create_cluster_cr(client: &Client) -> Result<(), Error> {
    let clusters: Api<Cluster> = Api::all(client.clone());
    let spec = ClusterSpec {
        machine_type: "pc-q35".into(),
        cpu: "<cpu/>".into(),
    };
    let created = clusters
        .create(
            &PostParams::default(),
            &Cluster::new("default", spec.clone()),
        )
        .await?;
    assert_eq!(created.spec, spec);
    Ok(())
}

async fn create_namespace(client: &Client, namespace: &str) -> Result<(), Error> {
    Api::<Namespace>::all(client.clone())
        .create(
            &PostParams::default(),
            &Namespace {
                metadata: ObjectMeta {
                    name: Some(namespace.into()),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .await?;
    Ok(())
}

async fn create_controller_deployment(
    client: &Client,
    namespace: &str,
    image: &str,
) -> Result<(), Error> {
    let controller: Deployment = serde_json::from_value(json!({
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": { "name": "cluster-controller", "namespace": namespace },
        "spec": {
            "selector": { "matchLabels": { "app": "cluster-controller" } },
            "template": {
                "metadata": { "labels": { "app": "cluster-controller" } },
                "spec": { "containers": [{ "name": "cluster-controller", "image": image }] }
            }
        }
    }))?;
    Api::<Deployment>::namespaced(client.clone(), namespace)
        .create(&PostParams::default(), &controller)
        .await?;
    Ok(())
}
