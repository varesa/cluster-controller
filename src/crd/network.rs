use crate::errors::Error;
use crate::utils::wait_crd_ready;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::{
    api::{Patch, PatchParams}, Api, Client, CustomResource,
    CustomResourceExt,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::instrument;

const NETWORK_CRD_NAME: &str = "networks.cluster-virt.acl.fi";

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

#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub enum NetworkType {
    #[default]
    Ovn,
    Evpn,
}

mod latest {
    //pub const VERSION: &str = "v1beta1";

    pub type Network = super::v1beta1::Network;
    pub type NetworkStatus = super::v1beta1::NetworkStatus;
}

pub mod v1beta1 {
    use super::*;
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
        /// Configure DHCP service for this network
        pub dhcp: Option<DhcpOptions>,
        /// Connect routers to this network
        pub routers: Option<Vec<RouterAttachment>>,
        /// Type of network
        ///
        /// Allowed values:
        /// - Ovn (default)
        /// - Evpn
        #[serde(skip_serializing_if = "Option::is_none")]
        pub network_type: Option<NetworkType>,
        /// Id of the network.
        ///
        /// Meaning depends on the network type, like VLAN ID, EVPN VNI, etc.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub network_id: Option<usize>,
        ///  Bridge that the network exists on
        #[serde(skip_serializing_if = "Option::is_none")]
        pub bridge: Option<String>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
    pub struct NetworkStatus {
        pub is_created: bool,
    }
}

#[instrument(skip(client))]
pub async fn create(client: Client) -> Result<(), Error> {
    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());
    let patch_params = PatchParams::apply("cluster-manager.ceph").force();

    let network_crd = Network::crd();
    crds.patch(NETWORK_CRD_NAME, &patch_params, &Patch::Apply(&network_crd))
        .await?;
    wait_crd_ready(&crds, NETWORK_CRD_NAME).await?;
    Ok(())
}

pub(crate) type Network = latest::Network;
pub(crate) type NetworkStatus = latest::NetworkStatus;
