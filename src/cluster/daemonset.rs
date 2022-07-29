use k8s_openapi::api::apps::v1::DaemonSet;
use serde_json::json;

use crate::errors::Error;

pub fn make_daemonset(image: String) -> Result<DaemonSet, Error> {
    let ds: DaemonSet = serde_json::from_value(json!({
      "apiVersion": "apps/v1",
      "kind": "DaemonSet",
      "metadata": {
        "name": "libvirt-host-controller",
        "labels": {}
      },
      "spec": {
        "selector": {
          "matchLabels": {
            "name": "libvirt-host-controller"
          }
        },
        "template": {
          "metadata": {
            "labels": {
              "name": "libvirt-host-controller"
            }
          },
          "spec": {
            "containers": [
              {
                "name": "libvirt-host-controller",
                "image": image,
                "command": ["cluster-controller", "--host"],
                "env": [
                  {
                    "name": "NODE_NAME",
                    "valueFrom": {
                      "fieldRef": { "fieldPath": "spec.nodeName" }
                    }
                  }
                ],
                "volumeMounts": [
                  {
                    "name": "libvirt-sock",
                    "mountPath": "/var/run/libvirt/libvirt-sock"
                  },
                  {
                    "name": "ceph-config",
                    "mountPath": "/etc/ceph"
                  }
                ]
              }
            ],
            "volumes": [
              {
                "name": "libvirt-sock",
                "hostPath": {
                  "path": "/var/run/libvirt/libvirt-sock"
                }
              },
              {
                "name": "ceph-config",
                "hostPath": {
                  "path": "/etc/ceph"
                }
              }
            ],
          }
        }
      }
    }))?;
    Ok(ds)
}
