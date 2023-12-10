use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::ServiceAccount;
use k8s_openapi::api::rbac::v1::{ClusterRole, ClusterRoleBinding, PolicyRule, RoleRef, Subject};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{Patch, PatchParams};
use kube::{Api, Client, ResourceExt};
use serde_json::json;
use tracing::instrument;

use crate::cluster::get_running_image;
use crate::Error;

fn make_service_account(namespace: &str) -> ServiceAccount {
    ServiceAccount {
        metadata: ObjectMeta {
            namespace: Some(String::from(namespace)),
            name: Some(String::from("metadata-service")),
            ..ObjectMeta::default()
        },
        ..ServiceAccount::default()
    }
}

fn make_cluster_role() -> ClusterRole {
    ClusterRole {
        metadata: ObjectMeta {
            name: Some(String::from("metadata-service")),
            ..ObjectMeta::default()
        },
        rules: Some(vec![
            PolicyRule {
                api_groups: Some(vec![String::from("cluster-virt.acl.fi")]),
                resources: Some(vec![String::from("virtualmachines")]),
                verbs: vec![String::from("list")],
                ..PolicyRule::default()
            },
            PolicyRule {
                api_groups: Some(vec![String::from("")]),
                resources: Some(vec![String::from("configmaps")]),
                verbs: vec![String::from("get")],
                ..PolicyRule::default()
            },
        ]),
        ..ClusterRole::default()
    }
}

fn make_cluster_role_binding(namespace: &str) -> ClusterRoleBinding {
    ClusterRoleBinding {
        metadata: ObjectMeta {
            name: Some(format!("metadata-service-{}", namespace)),
            ..ObjectMeta::default()
        },
        role_ref: RoleRef {
            api_group: "rbac.authorization.k8s.io".into(),
            kind: "ClusterRole".into(),
            name: "metadata-service".into(),
        },
        subjects: Some(vec![Subject {
            kind: "ServiceAccount".into(),
            name: "metadata-service".into(),
            namespace: Some(String::from(namespace)),
            ..Subject::default()
        }]),
    }
}

pub fn make_deployment(image: &str, namespace: &str, router: &str) -> Result<Deployment, Error> {
    let ds: Deployment = serde_json::from_value(json!({
      "apiVersion": "apps/v1",
      "kind": "Deployment",
      "metadata": {
        "name": format!("metadata-{}-{}", namespace, router),
        "labels": {}
      },
      "spec": {
        "selector": {
          "matchLabels": {
            "name": format!("metadata-{}-{}", namespace, router)
          }
        },
        "strategy": {
          "type": "Recreate"
        },
        "template": {
          "metadata": {
            "labels": {
              "name": format!("metadata-{}-{}", namespace, router)
            }
          },
          "spec": {
            "dnsPolicy": "ClusterFirstWithHostNet",
            "hostNetwork": true,
            "containers": [
              {
                "name": format!("metadata-{}-{}", namespace, router),
                "image": image,
                "command": ["cluster-controller", "--metadata-service", format!("{}/{}", namespace, router)],
                "env": [
                  {
                    "name": "NODE_NAME",
                    "valueFrom": {
                      "fieldRef": { "fieldPath": "spec.nodeName" }
                    }
                  }
                ],
                "securityContext": {
                  "privileged": true
                },
                "volumeMounts": [
                    {
                        "name": "ovsdb",
                        "mountPath": "/var/run/openvswitch/db.sock"
                    }
                ]
              }
            ],
            "serviceAccountName": "metadata-service",
            "volumes": [
                {
                    "name": "ovsdb",
                    "hostPath": {
                        "path": "/var/run/openvswitch/db.sock"
                    }
                }
            ]
          }
        }
      }
    }))?;
    Ok(ds)
}

#[instrument(skip(client))]
pub async fn create_rbac(client: Client, namespace: &str, controller: &str) -> Result<(), Error> {
    let service_accounts: Api<ServiceAccount> = Api::namespaced(client.clone(), namespace);
    let sa = make_service_account(namespace);
    service_accounts
        .patch(
            &sa.name_unchecked(),
            &PatchParams::apply(controller),
            &Patch::Apply(sa),
        )
        .await?;

    let cluster_roles: Api<ClusterRole> = Api::all(client.clone());
    let cr = make_cluster_role();
    cluster_roles
        .patch(
            &cr.name_unchecked(),
            &PatchParams::apply(controller),
            &Patch::Apply(cr),
        )
        .await?;

    let cluster_role_bindings: Api<ClusterRoleBinding> = Api::all(client.clone());
    let crb = make_cluster_role_binding(namespace);
    cluster_role_bindings
        .patch(
            &crb.name_unchecked(),
            &PatchParams::apply(controller),
            &Patch::Apply(crb),
        )
        .await?;
    Ok(())
}

#[instrument(skip(client))]
pub async fn create_deployment(
    client: Client,
    controller: &str,
    namespace: &str,
    router: &str,
) -> Result<(), Error> {
    let deployments: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let metadataservice_deploy =
        make_deployment(&get_running_image(client.clone()).await?, namespace, router)?;
    deployments
        .patch(
            metadataservice_deploy.metadata.name.as_ref().unwrap(),
            &PatchParams::apply(controller),
            &Patch::Apply(&metadataservice_deploy),
        )
        .await?;
    Ok(())
}

#[instrument(skip(client))]
pub async fn deploy(
    client: Client,
    controller: &str,
    namespace: &str,
    router: &str,
) -> Result<(), Error> {
    create_rbac(client.clone(), namespace, controller).await?;
    create_deployment(client.clone(), controller, namespace, router).await?;
    Ok(())
}
