mod daemonset;
mod ceph;

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

pub async fn run(client: Client, namespace: &str) -> Result<(), Error> {
    let daemonsets: Api<DaemonSet> = Api::namespaced(client.clone(), namespace);

    // Create libvirt host controllers
    let libvirt_ds = daemonset::make_daemonset("hello:world4".into())?;
    daemonsets.patch("libvirt-host-controller", &PatchParams::apply("libvirt-controller-cluster"), &Patch::Apply(&libvirt_ds)).await?;
    println!("daemonset/libvirt-host-controller: OK");

    // Create ceph cluster controller
    ceph::run(client).await?;
    println!("ceph cluster controller: OK");
    Ok(())
}