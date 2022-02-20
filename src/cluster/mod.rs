mod ceph;
mod daemonset;
mod libvirt;
mod ovn;

use crate::errors::Error;
use crate::{crd, NAMESPACE};
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment};
use kube::{
    api::{Patch, PatchParams},
    Api, Client,
};

const DEPLOYMENT_NAME: &str = "cluster-controller";

async fn get_running_image(kube: Client) -> Result<String, Error> {
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

    // Create ceph cluster controller
    let client_clone = client.clone();
    let ceph_task = tokio::task::spawn(async {
        panic!("Ceph task exited: {:?}", ceph::run(client_clone).await);
    });

    // Create libvirt cluster controller
    let client_clone = client.clone();
    let libvirt_task = tokio::task::spawn(async {
        panic!(
            "Libvirt task exited: {:?}",
            libvirt::run(client_clone).await
        );
    });

    // Create libvirt cluster controller
    let client_clone = client.clone();
    let ovn_task = tokio::task::spawn(async {
        panic!("OVN task exited: {:?}", ovn::run(client_clone).await);
    });

    let _ = tokio::try_join!(ceph_task, libvirt_task, ovn_task);
    panic!("One of the controllers exited");
}
