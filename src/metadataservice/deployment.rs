use k8s_openapi::api::apps::v1::Deployment;
use serde_json::json;

use crate::Error;

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
                }
              }
            ]
          }
        }
      }
    }))?;
    Ok(ds)
}
