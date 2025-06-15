use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::Resource;
use kube::ResourceExt;
use kube::api::PostParams;
use kube::core::crd::merge_crds;
use kube::{
    Api, Client, CustomResource, CustomResourceExt,
    api::{Patch, PatchParams},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::info;

use crate::errors::Error;
use crate::utils::wait_crd_ready;
use tracing::instrument;

const CRD_NAME: &str = "libvirtnodes.cluster-virt.acl.fi";

mod latest {
    pub const VERSION: &str = "v1beta1";

    pub type LibvirtNode = super::v1beta1::LibvirtNode;
    pub type LibvirtNodeStatus = super::v1beta1::LibvirtNodeStatus;
}

pub mod v1beta1 {
    use super::*;

    #[derive(
        CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema,
    )]
    #[kube(
        group = "cluster-virt.acl.fi",
        version = "v1beta1",
        kind = "LibvirtNode",
        status = "LibvirtNodeStatus",
        derive = "PartialEq",
        derive = "Default",
        shortname = "lvnode"
    )]
    pub struct LibvirtNodeSpec {}

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, Default)]
    pub struct LibvirtNodeStatus {
        pub capabilities: String,
    }
}

#[instrument(skip(client))]
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

            let latest_crd = latest::LibvirtNode::crd();
            crds.patch(CRD_NAME, &patch_params, &Patch::Apply(&latest_crd))
                .await?;

            return Ok(());
        }
    }

    // Create all possible versions to exist in parallel
    let crd_versions = vec![v1beta1::LibvirtNode::crd()];
    let crd = merge_crds(crd_versions, latest::VERSION)?;
    crds.patch(CRD_NAME, &patch_params, &Patch::Apply(&crd))
        .await?;
    wait_crd_ready(&crds, CRD_NAME).await?;

    // Migrate all objects to the latest version
    run_migrations(client.clone()).await?;

    // Remove all but the latest CRD version
    let latest_crd = latest::LibvirtNode::crd();
    crds.patch(CRD_NAME, &patch_params, &Patch::Apply(&latest_crd))
        .await?;

    Ok(())
}

#[instrument(skip(_client))]
pub async fn run_migrations(_client: Client) -> Result<(), Error> {
    Ok(())
}

pub(crate) type LibvirtNode = latest::LibvirtNode;
pub(crate) type LibvirtNodeStatus = latest::LibvirtNodeStatus;

create_set_status_cluster_scoped!(LibvirtNode, LibvirtNodeStatus, set_libvirtnode_status);
