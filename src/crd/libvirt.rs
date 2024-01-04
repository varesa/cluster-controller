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
use tracing::{debug, info};

use crate::crd::utils;
use crate::errors::Error;
use crate::utils::{get_namespace_names, wait_crd_ready};
use tracing::instrument;

const CRD_NAME: &str = "virtualmachines.cluster-virt.acl.fi";

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema)]
pub struct VolumeAttachment {
    pub name: String,
}

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema)]
pub struct NetworkAttachment {
    // Allow specification of a managed Network instance
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    // Or an externally created host bridge device
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bridge: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub mac_address: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ovn_id: Option<String>,
}

mod latest {
    pub const VERSION: &str = "v1beta3";

    pub type VirtualMachine = super::v1beta3::VirtualMachine;
    pub type VirtualMachineStatus = super::v1beta3::VirtualMachineStatus;
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
        printcolumn = r#"{"name":"Node", "type":"string", "description":"Node the VM is scheduled to", "jsonPath":".status.node"}"#,
        printcolumn = r#"{"name":"IPs", "type":"string", "description":"Dynamic IPs assigned", "jsonPath":".status.ip_addresses_string"}"#
    )]
    pub struct VirtualMachineSpec {
        pub cpus: usize,
        // String to allow suffixes like '1 Gi'
        pub memory: String,
        pub volumes: Vec<VolumeAttachment>,
        pub networks: Vec<NetworkAttachment>,
        pub uuid: Option<String>,
        pub userdata: Option<String>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
    pub struct VirtualMachineStatus {
        pub scheduled: bool,
        pub running: bool,
        pub migration_pending: bool,
        pub node: Option<String>,
        pub domain_name: String,
        pub ip_addresses: Option<Vec<String>>,
        pub ip_addresses_string: Option<String>,
    }
}

pub mod v1beta3 {
    use super::*;

    #[derive(
        CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema,
    )]
    #[kube(
        group = "cluster-virt.acl.fi",
        version = "v1beta3",
        kind = "VirtualMachine",
        status = "VirtualMachineStatus",
        derive = "PartialEq",
        derive = "Default",
        shortname = "vm",
        namespaced,
        printcolumn = r#"{"name":"Node", "type":"string", "description":"Node the VM is scheduled to", "jsonPath":".status.node"}"#,
        printcolumn = r#"{"name":"IPs", "type":"string", "description":"Dynamic IPs assigned", "jsonPath":".status.ip_addresses_string"}"#
    )]
    pub struct VirtualMachineSpec {
        pub cpus: usize,
        // String to allow suffixes like '1 Gi'
        pub memory: String,
        pub volumes: Vec<VolumeAttachment>,
        pub networks: Vec<NetworkAttachment>,
        pub uuid: Option<String>,
        pub userdata: Option<String>,
        pub compatibility_mode: Option<bool>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
    pub struct VirtualMachineStatus {
        pub scheduled: bool,
        pub running: bool,
        pub migration_pending: bool,
        pub node: Option<String>,
        pub domain_name: String,
        pub ip_addresses: Option<Vec<String>>,
        pub ip_addresses_string: Option<String>,
        pub networks: Vec<NetworkAttachment>,
    }
}

pub async fn create(client: Client) -> Result<(), Error> {
    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());
    let patch_params = PatchParams::apply("cluster-manager.libvirt").force();

    if let Ok(crd) = crds.get(CRD_NAME).await {
        let current_versions: Vec<String> = crd
            .spec
            .versions
            .into_iter()
            .map(|version| version.name)
            .collect();
        if current_versions == [String::from(latest::VERSION)] {
            info!("CRD virtualmachines: no migrations required");

            let latest_crd = latest::VirtualMachine::crd();
            crds.patch(CRD_NAME, &patch_params, &Patch::Apply(&latest_crd))
                .await?;

            return Ok(());
        }
    }

    // Create all possible versions to exist in parallel
    let crd_versions = vec![
        v1beta2::VirtualMachine::crd(),
        v1beta3::VirtualMachine::crd(),
    ];
    let crd = merge_crds(crd_versions, latest::VERSION)?;
    crds.patch(CRD_NAME, &patch_params, &Patch::Apply(&crd))
        .await?;
    wait_crd_ready(&crds, CRD_NAME).await?;

    // Migrate all objects to the latest version
    run_migrations(client.clone()).await?;

    // Remove all but the latest CRD version
    let latest_crd = latest::VirtualMachine::crd();
    crds.patch(CRD_NAME, &patch_params, &Patch::Apply(&latest_crd))
        .await?;

    Ok(())
}

pub async fn run_migrations(client: Client) -> Result<(), Error> {
    const CRD_NAME: &str = "virtualmachines.cluster-virt.acl.fi";
    let patch_params = PatchParams::apply("cluster-manager.libvirt");
    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());
    let vm_crd = crds.get(CRD_NAME).await?;

    let status = vm_crd.status.expect("CRD has no status").clone();

    for version in status.stored_versions.clone().unwrap_or_default() {
        debug!("CRD virtualmachines: checking version {}", &version);
        if &version == "v1beta2" {
            info!("CRD virtualmachines: upgrading {}", &version);
            for namespace in get_namespace_names(client.clone()).await? {
                let api_v1beta2: Api<v1beta2::VirtualMachine> =
                    Api::namespaced(client.clone(), &namespace);
                let api_v1beta3: Api<v1beta3::VirtualMachine> =
                    Api::namespaced(client.clone(), &namespace);

                let vms_v1beta2 = api_v1beta2.list(&ListParams::default()).await?;
                for vm in vms_v1beta2 {
                    debug!(
                        "CRD virtualmachines: Upgrading {} {}/{}",
                        &version,
                        &namespace,
                        &vm.metadata.name.as_ref().unwrap()
                    );
                    let patch = json!({
                        "status": {
                            "networks": &vm.spec.networks,
                        }
                    });
                    api_v1beta3
                        .patch_status(
                            vm.metadata.name.as_ref().expect("vm has no name"),
                            &patch_params,
                            &Patch::Merge(patch),
                        )
                        .await?;
                }
            }
            debug!("CRD virtualmachines: removing version {}", &version);
            utils::remove_crd_version(CRD_NAME, &version, client.clone()).await?;
        }
    }

    Ok(())
}

pub(crate) type VirtualMachine = latest::VirtualMachine;
pub(crate) type VirtualMachineStatus = latest::VirtualMachineStatus;

create_set_status!(VirtualMachine, VirtualMachineStatus, set_vm_status);
