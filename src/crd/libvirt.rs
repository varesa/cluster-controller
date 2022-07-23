use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::api::ListParams;
use kube::core::crd::merge_crds;
use kube::{
    api::{Patch, PatchParams, PostParams},
    Api, Client, CustomResource, CustomResourceExt, Resource, ResourceExt,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::crd::utils;
use crate::errors::Error;
use crate::utils::{get_namespace_names, wait_crd_ready};

#[derive(Debug, PartialEq, Eq, Clone, JsonSchema, Serialize, Deserialize, Default)]
pub struct Quantity(String);

const CRD_NAME: &str = "virtualmachines.cluster-virt.acl.fi";

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema)]
pub struct VolumeAttachment {
    pub name: String,
}

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema)]
pub struct NetworkAttachment {
    // Allow specification of a managed Network instance
    pub name: Option<String>,
    // Or an externally created host bridge device
    pub bridge: Option<String>,

    pub mac_address: Option<String>,
    pub ovn_id: Option<String>,
}

mod v1beta {
    use super::*;

    #[derive(
        CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema,
    )]
    #[kube(
        group = "cluster-virt.acl.fi",
        version = "v1beta",
        kind = "VirtualMachine",
        status = "VirtualMachineStatus",
        derive = "PartialEq",
        derive = "Default",
        shortname = "vm",
        namespaced,
        printcolumn = r#"{"name":"Node", "type":"string", "description":"Node the VM is scheduled to", "jsonPath":".status.node"}"#
    )]
    pub struct VirtualMachineSpec {
        pub cpus: usize,
        // String to allow suffixes like '1 Gi'
        pub memory: String,
        pub volumes: Vec<VolumeAttachment>,
        pub networks: Vec<NetworkAttachment>,
        pub uuid: Option<String>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
    pub struct VirtualMachineStatus {
        pub scheduled: bool,
        pub running: bool,
        pub node: Option<String>,
        pub domain_name: String,
    }
}

pub mod v1beta2 {
    use super::*;

    #[derive(
        CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema,
    )]
    #[kube(
        group = "cluster-virt.acl.fi",
        version = "v1beta2",
        kind = "VirtualMachine",
        status = "VirtualMachineStatus",
        derive = "PartialEq",
        derive = "Default",
        shortname = "vm",
        namespaced,
        printcolumn = r#"{"name":"Node", "type":"string", "description":"Node the VM is scheduled to", "jsonPath":".status.node"}"#
    )]
    pub struct VirtualMachineSpec {
        pub cpus: usize,
        // String to allow suffixes like '1 Gi'
        pub memory: String,
        pub volumes: Vec<VolumeAttachment>,
        pub networks: Vec<NetworkAttachment>,
        pub uuid: Option<String>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
    pub struct VirtualMachineStatus {
        pub scheduled: bool,
        pub running: bool,
        pub migration_pending: bool,
        pub node: Option<String>,
        pub domain_name: String,
    }
}

pub async fn create(client: Client) -> Result<(), Error> {
    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());
    let patch_params = PatchParams::apply("cluster-manager.libvirt").force();

    let crd_versions = vec![
        v1beta::VirtualMachine::crd(),
        v1beta2::VirtualMachine::crd(),
    ];
    let crd = merge_crds(crd_versions, "v1beta2")?;
    crds.patch(CRD_NAME, &patch_params, &Patch::Apply(&crd))
        .await?;
    wait_crd_ready(&crds, CRD_NAME).await?;
    run_migrations(client.clone()).await?;
    Ok(())
}

pub async fn run_migrations(client: Client) -> Result<(), Error> {
    const CRD_NAME: &str = "virtualmachines.cluster-virt.acl.fi";
    let patch_params = PatchParams::apply("cluster-manager.libvirt");
    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());
    let vm_crd = crds.get(CRD_NAME).await?;

    let status = vm_crd.status.expect("CRD has no status").clone();

    for version in status.stored_versions.clone().unwrap_or_default() {
        if &version == "v1beta" {
            for namespace in get_namespace_names(client.clone()).await? {
                let api_v1beta: Api<v1beta::VirtualMachine> =
                    Api::namespaced(client.clone(), &namespace);
                let api_v1beta2: Api<v1beta2::VirtualMachine> =
                    Api::namespaced(client.clone(), &namespace);

                let vms_v1beta = api_v1beta.list(&ListParams::default()).await?;
                for vm in vms_v1beta {
                    let patch = json!({
                        "status": {
                            "migration_pending": false,
                        }
                    });
                    api_v1beta2
                        .patch_status(
                            vm.metadata.name.as_ref().expect("vm has no name"),
                            &patch_params,
                            &Patch::Merge(patch),
                        )
                        .await?;
                }
            }
            utils::remove_crd_version(CRD_NAME, "v1beta", client.clone()).await?;
        }
    }

    Ok(())
}

type Vm = v1beta2::VirtualMachine;
type VmStatus = v1beta2::VirtualMachineStatus;

create_set_status!(Vm, VmStatus, set_vm_status);
