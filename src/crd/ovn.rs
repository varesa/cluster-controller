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

const NETWORK_CRD_NAME: &str = "networks.cluster-virt.acl.fi";
const ROUTER_CRD_NAME: &str = "routers.cluster-virt.acl.fi";

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema)]
pub struct DhcpOptions {
    pub cidr: String,
    pub lease_time: Option<u64>,
    pub dns_server: Option<String>,
    pub domain_name: Option<String>,
    pub router: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema)]
pub struct RouterAttachment {
    pub name: String,
    pub address: String,
}

#[derive(
    CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema,
)]
#[kube(
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
    pub dhcp: Option<DhcpOptions>,
    pub routers: Option<Vec<RouterAttachment>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct NetworkStatus {
    pub is_created: bool,
}

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

    let network_crd = Network::crd();
    crds.patch(NETWORK_CRD_NAME, &patch_params, &Patch::Apply(&network_crd))
        .await?;
    let router_crd = Router::crd();
    crds.patch(ROUTER_CRD_NAME, &patch_params, &Patch::Apply(&router_crd))
        .await?;
    wait_crd_ready(&crds, NETWORK_CRD_NAME).await?;
    wait_crd_ready(&crds, ROUTER_CRD_NAME).await?;
    Ok(())
}
