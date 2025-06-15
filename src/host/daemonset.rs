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
        "updateStrategy": {
          "type": "RollingUpdate",
          "rollingUpdate": {
            "maxUnavailable": 3
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
                  },
                  {
                    "name": "RUST_LOG",
                    "value": "cluster_controller=debug"
                  },
                  {
                    "name": "OTLP_ENDPOINT",
                    "value": "http://10.4.131.101:4317"
                  }
                ],
                "volumeMounts": [
                  {
                    "name": "libvirt-sock",
                    "mountPath": "/var/run/libvirt/libvirt-sock"
                  },
                  {
                    "name": "virtqemud-sock",
                    "mountPath": "/var/run/libvirt/virtqemud-sock"
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
                "name": "virtqemud-sock",
                "hostPath": {
                  "path": "/var/run/libvirt/virtqemud-sock"
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
