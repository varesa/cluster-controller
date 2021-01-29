use k8s_openapi::{
    apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition,
    Resource
};
use kube::{
    api::{
        ListParams,
        Patch,
        PatchParams,
        WatchEvent,
    },
    Api,
    Client,
    CustomResource,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use schemars::JsonSchema;
use crate::errors::Error;
use futures::{StreamExt, TryStreamExt};

#[derive(Debug, PartialEq, Clone, JsonSchema, Serialize, Deserialize, Default)]
struct Quantity(String);

const CRD_NAME: &str = "volumes.cluster-virt.acl.fi";

#[derive(CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Clone, JsonSchema)]
#[kube(
    apiextensions = "v1",
    group = "cluster-virt.acl.fi",
    version = "v1beta",
    kind = "Volume",
    status = "VolumeStatus",
    derive = "PartialEq",
    derive = "Default",
    shortname = "v",
    namespaced,
)]
pub struct VolumeSpec {
    //name: String,
    size: Quantity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct VolumeStatus {
    is_created: bool,
}

pub async fn create(client: Client) -> Result<(), Error> {
    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());
    let patch_params = PatchParams::apply("cluster-manager.ceph").force();

    let crd = Volume::crd();
    //println!("Creating CRD: {}", serde_json::to_string_pretty(&crd).expect("Failed to convert CRD to JSON"));
    crds.patch(CRD_NAME, &patch_params, &Patch::Apply(&crd)).await?;
    wait_crd_ready(&crds, CRD_NAME).await?;
    Ok(())
}

async fn wait_crd_ready(crds: &Api<CustomResourceDefinition>, name: &str) -> Result<(), Error> {
    if crds.get(name).await.is_ok() {
        return Ok(());
    }

    let list_params = ListParams::default()
        .fields(&format!("metdata.name={}", name))
        .timeout(5);
    let mut stream = crds.watch(&list_params, "0").await?.boxed();

    while let Some(status) = stream.try_next().await? {
        if let WatchEvent::Modified(crd) = status {
            println!("Modify event for {}", name);
            if let Some(status) = crd.status {
                if let Some(conditions) = status.conditions {
                    if let Some(pcond) = conditions.iter().find(|c| c.type_ == "NamesAccepted") {
                        if pcond.status == "True" {
                            println!("CRD accepted: {}", name);
                            return Ok(());
                        }
                    }
                }
            }
        }
    }
    return Err(Error::Timeout(format!("Apply CRD {}", name)));
}