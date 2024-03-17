use k8s_openapi::api::apps::v1::{DaemonSet, Deployment};
use kube::{
    api::{Patch, PatchParams},
    Api, Client,
};
use tracing::error;

use crate::errors::Error;
use crate::host::daemonset;
use crate::{crd, NAMESPACE};

mod controllers;
mod libvirt;
pub mod ovn;

const DEPLOYMENT_NAME: &str = "cluster-controller";

pub const MAINTENANCE_ANNOTATION: &str = "cluster-virt.acl.fi/maintenance";

pub const NO_SCHEDULE_ANNOTATION: &str = "cluster-virt.acl.fi/no-schedule";
pub const MIGRATION_REQUEST_ANNOTATION: &str = "cluster-virt.acl.fi/migration-required";

pub async fn get_running_image(kube: Client) -> Result<String, Error> {
    let deployments: Api<Deployment> = Api::namespaced(kube, NAMESPACE);
    let deployment = deployments.get(DEPLOYMENT_NAME).await?;
    let image = deployment.spec.unwrap().template.spec.unwrap().containers[0]
        .image
        .as_ref()
        .unwrap()
        .to_owned();
    Ok(image)
}

pub async fn run(client: Client, namespace: &str) -> Result<(), Error> {
    tracing::info!("Running in cluster mode");
    let daemonsets: Api<DaemonSet> = Api::namespaced(client.clone(), namespace);

    // Create cluster CRD
    crd::cluster::create(client.clone()).await?;

    // Create libvirt host controllers
    let image = get_running_image(client.clone()).await?;
    let libvirt_ds = daemonset::make_daemonset(image)?;
    daemonsets
        .patch(
            "libvirt-host-controller",
            &PatchParams::apply("libvirt-controller-cluster"),
            &Patch::Apply(&libvirt_ds),
        )
        .await?;

    let result = controllers::run(client).await;
    error!("supervisor: ERROR: One of the controllers died, killing the rest of the application: {result:#?}");
    std::process::exit(1);
}
