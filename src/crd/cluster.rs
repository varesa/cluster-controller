use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::{
    api::{Patch, PatchParams},
    Api, Client, CustomResource, CustomResourceExt,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::errors::Error;
use crate::utils::wait_crd_ready;

const CRD_NAME: &str = "clusters.cluster-virt.acl.fi";

#[derive(
    CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema,
)]
#[kube(
    group = "cluster-virt.acl.fi",
    version = "v1beta",
    kind = "Cluster",
    status = "ClusterStatus",
    derive = "PartialEq",
    derive = "Default"
)]
pub struct ClusterSpec {
    // e.g. pc-q35-rhel8.3.0
    pub machine_type: String,

    // <cpu>...</cpu>
    pub cpu: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct ClusterStatus {}

#[instrument(skip(client))]
pub async fn create(client: Client) -> Result<(), Error> {
    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());
    let patch_params = PatchParams::apply("virt-controller").force();

    let crd = Cluster::crd();
    crds.patch(CRD_NAME, &patch_params, &Patch::Apply(&crd))
        .await?;
    wait_crd_ready(&crds, CRD_NAME).await?;
    Ok(())
}
