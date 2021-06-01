use k8s_openapi::{
    apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition,
};
use kube::{
    api::{
        Patch,
        PatchParams,
    },
    Api,
    Client,
    CustomResource,
};
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;

use crate::errors::Error;
use crate::utils::wait_crd_ready;

#[derive(Debug, PartialEq, Clone, JsonSchema, Serialize, Deserialize, Default)]
pub struct Quantity(String);

const CRD_NAME: &str = "clusters.cluster-virt.acl.fi";

#[derive(CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Clone, JsonSchema)]
#[kube(
apiextensions = "v1",
group = "cluster-virt.acl.fi",
version = "v1beta",
kind = "Cluster",
status = "ClusterStatus",
derive = "PartialEq",
derive = "Default",
)]
pub struct ClusterSpec {
    // e.g. pc-q35-rhel8.3.0
    pub machine_type: String,

    // <cpu>...</cpu>
    pub cpu: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ClusterStatus {}

pub async fn create(client: Client) -> Result<(), Error> {
    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());
    let patch_params = PatchParams::apply("virt-controller").force();

    let crd = Cluster::crd();
    crds.patch(CRD_NAME, &patch_params, &Patch::Apply(&crd)).await?;
    wait_crd_ready(&crds, CRD_NAME).await?;
    Ok(())
}