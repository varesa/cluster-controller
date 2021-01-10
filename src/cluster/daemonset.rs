use std::collections::BTreeMap;
use k8s_openapi::api::apps::v1::{DaemonSet, DaemonSetSpec};
use serde_json::json;
//use serde_yaml::yaml;
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
              }
            ],
          }
        }
      }
    }))?;
    Ok(ds)
}