mod daemonset;
mod ceph;
mod libvirt;

use kube::{
    api::{
        Patch,
        PatchParams,
    },
    Api,
    Client,
};
use k8s_openapi::api::apps::v1::{DaemonSet};
use crate::errors::Error;
use tokio::time::Duration;
use crate::utils::get_version_string;
use crate::crd;

pub async fn run(client: Client, namespace: &str) -> Result<(), Error> {
    let daemonsets: Api<DaemonSet> = Api::namespaced(client.clone(), namespace);

    // Create cluster CRD
    crd::cluster::create(client.clone()).await?;

    // Create libvirt host controllers
    let libvirt_ds = daemonset::make_daemonset(format!("registry.acl.fi/public/virt-controller:{}", get_version_string()))?;
    daemonsets.patch("libvirt-host-controller", &PatchParams::apply("libvirt-controller-cluster"), &Patch::Apply(&libvirt_ds)).await?;

    // Create ceph cluster controller
    let client_clone = client.clone();
    tokio::task::spawn(async  {
        panic!("Ceph task exited: {:?}", ceph::run(client_clone).await);
    });

    // Create libvirt cluster controller
    let client_clone = client.clone();
    tokio::task::spawn(async {
        panic!("Libvirt task exited: {:?}", libvirt::run(client_clone).await);
    });

    loop {
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}
