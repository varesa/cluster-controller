use crate::errors::Error;
use crate::utils::wait_crd_ready;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::{
    Api, Client, CustomResource, CustomResourceExt,
    api::{Patch, PatchParams},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::instrument;

const ROUTER_CRD_NAME: &str = "routers.cluster-virt.acl.fi";

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema)]
pub struct Route {
    pub cidr: String,
    pub nexthop: String,
}

#[derive(
    CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema,
)]
#[kube(
    group = "cluster-virt.acl.fi",
    version = "v1beta",
    kind = "Router",
    status = "RouterStatus",
    derive = "PartialEq",
    derive = "Default",
    shortname = "r",
    namespaced
)]
pub struct RouterSpec {
    pub routes: Option<Vec<Route>>,
    pub metadata_service: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct RouterStatus {
    pub is_created: bool,
}

#[instrument(skip(client))]
pub async fn create(client: Client) -> Result<(), Error> {
    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());
    let patch_params = PatchParams::apply("cluster-manager.ceph").force();

    let router_crd = Router::crd();
    crds.patch(ROUTER_CRD_NAME, &patch_params, &Patch::Apply(&router_crd))
        .await?;
    wait_crd_ready(&crds, ROUTER_CRD_NAME).await?;
    Ok(())
}
