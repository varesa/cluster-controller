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
    pub volumes: Vec<String>,
    pub networks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct VirtualMachineStatus {
    is_created: bool,
}

pub async fn create(client: Client) -> Result<(), Error> {
    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());
    let patch_params = PatchParams::apply("cluster-manager.libvirt").force();

    let crd = VirtualMachine::crd();
    crds.patch(CRD_NAME, &patch_params, &Patch::Apply(&crd)).await?;
    wait_crd_ready(&crds, CRD_NAME).await?;
    Ok(())
}