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

const CRD_NAME: &str = "virtualmachines.cluster-virt.acl.fi";

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Clone, JsonSchema)]
pub struct VolumeAttachment {
    pub name: String,
}

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Clone, JsonSchema)]
pub struct NetworkAttachment {
    // Allow specification of a managed Network instance
    pub name: Option<String>,
    // Or an externally created host bridge device
    pub bridge: Option<String>,
    pub mac_address: Option<String>,

}

#[derive(CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Clone, JsonSchema)]
#[kube(
    apiextensions = "v1",
    group = "cluster-virt.acl.fi",
    version = "v1beta",
    kind = "VirtualMachine",
    status = "VirtualMachineStatus",
    derive = "PartialEq",
    derive = "Default",
    shortname = "vm",
    namespaced,
)]
pub struct VirtualMachineSpec {
    pub cpus: usize,
    // String to allow suffixes like '1 Gi'
    pub memory: String,
    pub volumes: Vec<VolumeAttachment>,
    pub networks: Vec<NetworkAttachment>,
    pub uuid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct VirtualMachineStatus {
    pub scheduled: bool,
    pub running: bool,
    pub node: Option<String>,
    pub domain_name: String,
}

pub async fn create(client: Client) -> Result<(), Error> {
    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());
    let patch_params = PatchParams::apply("cluster-manager.libvirt").force();

    let crd = VirtualMachine::crd();
    crds.patch(CRD_NAME, &patch_params, &Patch::Apply(&crd)).await?;
    wait_crd_ready(&crds, CRD_NAME).await?;
    Ok(())
}