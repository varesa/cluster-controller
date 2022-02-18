use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::{
    api::{Patch, PatchParams},
    Api, Client, CustomResource, CustomResourceExt,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::errors::Error;
use crate::utils::wait_crd_ready;

#[derive(Debug, PartialEq, Clone, JsonSchema, Serialize, Deserialize, Default)]
pub struct Quantity(String);

const CRD_NAME: &str = "networks.cluster-virt.acl.fi";

#[derive(CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Clone, JsonSchema)]
#[kube(
    apiextensions = "v1",
    group = "cluster-virt.acl.fi",
    version = "v1beta",
    kind = "Network",
    status = "NetworkStatus",
    derive = "PartialEq",
    derive = "Default",
    shortname = "n",
    namespaced
)]
pub struct NetworkSpec {
    // String to allow suffixes like '1 Gi'
//pub size: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct NetworkStatus {
    is_created: bool,
}

pub async fn create(client: Client) -> Result<(), Error> {
    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());
    let patch_params = PatchParams::apply("cluster-manager.ceph").force();

    let crd = Network::crd();
    crds.patch(CRD_NAME, &patch_params, &Patch::Apply(&crd))
        .await?;
    wait_crd_ready(&crds, CRD_NAME).await?;
    Ok(())
}