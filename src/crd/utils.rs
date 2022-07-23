use crate::{Client, Error};
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::api::{Patch, PatchParams};
use kube::Api;
use serde_json::json;

pub async fn remove_crd_version(
    crd_name: &str,
    crd_version: &str,
    client: Client,
) -> Result<(), Error> {
    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());
    let crd = crds.get(crd_name).await?;
    let mut versions = crd
        .status
        .expect("CRD has no status")
        .stored_versions
        .expect("CRD has no stored versions");

    versions.retain(|v| v != crd_version);

    let patch = json!({
        "status": {
            "storedVersions": versions
        }
    });

    crds.patch_status(
        crd_name,
        &PatchParams::apply("cluster-manager.libvirt"),
        &Patch::Merge(patch),
    )
    .await?;
    Ok(())
}
