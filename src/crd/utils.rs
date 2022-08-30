use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::api::{Patch, PatchParams, PostParams};
use kube::Api;
use serde_json::json;

use crate::{Client, Error};

pub async fn remove_crd_version(
    crd_name: &str,
    crd_version: &str,
    client: Client,
) -> Result<(), Error> {
    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());

    // Delete from .status.stored_versions

    let crd = crds.get(crd_name).await?;
    let mut stored_versions = crd
        .status
        .expect("CRD has no status")
        .stored_versions
        .expect("CRD has no stored versions");

    stored_versions.retain(|v| v != crd_version);

    let patch = json!({
        "status": {
            "storedVersions": stored_versions
        }
    });

    crds.patch_status(
        crd_name,
        &PatchParams::apply("cluster-manager.libvirt"),
        &Patch::Merge(patch),
    )
    .await?;

    // Delete from .spec.versions

    let mut crd = crds.get(crd_name).await?;
    crd.spec.versions.retain(|v| v.name != crd_version);
    crds.replace(crd_name, &PostParams::default(), &crd).await?;
    println!("CRD: Patching {:?}", &crd);
    Ok(())
}
