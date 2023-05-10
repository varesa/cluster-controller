use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::{
    api::{Patch, PatchParams},
    Api, Client, CustomResource, CustomResourceExt,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::errors::Error;
use crate::utils::wait_crd_ready;

#[derive(Debug, PartialEq, Eq, Clone, JsonSchema, Serialize, Deserialize, Default)]
pub struct Quantity(String);

const VOLUME_CRD_NAME: &str = "volumes.cluster-virt.acl.fi";
const IMAGE_CRD_NAME: &str = "images.cluster-virt.acl.fi";

#[derive(
    CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema,
)]
#[kube(
    group = "cluster-virt.acl.fi",
    version = "v1beta",
    kind = "Volume",
    status = "VolumeStatus",
    derive = "PartialEq",
    derive = "Default",
    shortname = "v",
    namespaced
)]
pub struct VolumeSpec {
    // String to allow suffixes like '1 Gi'
    pub size: String,
    pub template: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct VolumeStatus {
    is_created: bool,
}

#[derive(
    CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema,
)]
#[kube(
    group = "cluster-virt.acl.fi",
    version = "v1beta",
    kind = "Image",
    status = "ImageStatus",
    derive = "PartialEq",
    derive = "Default",
    shortname = "i",
    namespaced
)]
pub struct ImageSpec {
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct ImageStatus {
    size: usize,
    is_allocated: bool,
    is_imported: bool,
    import_in_progress: bool,
}

pub async fn create(client: Client) -> Result<(), Error> {
    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());
    let patch_params = PatchParams::apply("cluster-manager.ceph").force();

    let crd = Volume::crd();
    crds.patch(VOLUME_CRD_NAME, &patch_params, &Patch::Apply(&crd))
        .await?;
    wait_crd_ready(&crds, VOLUME_CRD_NAME).await?;

    let crd = Image::crd();
    crds.patch(IMAGE_CRD_NAME, &patch_params, &Patch::Apply(&crd))
        .await?;
    wait_crd_ready(&crds, IMAGE_CRD_NAME).await?;
    Ok(())
}
