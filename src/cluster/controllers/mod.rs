use crate::crd;
use crate::errors::Error;
use futures::try_join;
use kube::Client;
use log::info;
use ovn_services::{ovn_central, ovn_controller};

mod images;
mod network;
pub mod node;
mod ovn_services;
mod router;
mod virtualmachine;
mod volumes;

pub async fn run(client: Client) -> Result<(), Error> {
    info!("Creating CRDs");
    crd::libvirtnode::create(client.clone()).await?;
    crd::virtualmachine::create(client.clone()).await?;
    crd::ceph::create(client.clone()).await?;
    crd::network::create(client.clone()).await?;
    crd::router::create(client.clone()).await?;

    info!("Creating tasks");
    let volumes_task = tokio::spawn(volumes::create(client.clone()));
    let images_task = tokio::spawn(images::create(client.clone()));
    let ovn_controller_task = tokio::spawn(ovn_controller::create(client.clone()));
    let ovn_central_task = tokio::spawn(ovn_central::create(client.clone()));

    let network_task = tokio::spawn(network::create(client.clone()));
    let router_task = tokio::spawn(router::create(client.clone()));

    let vm_task1 = tokio::spawn(virtualmachine::ovn::create(client.clone()));
    let vm_task2 = tokio::spawn(virtualmachine::vm::create(client.clone()));

    let node_task = tokio::spawn(node::create(client.clone()));

    try_join!(
        async { volumes_task.await.unwrap() },
        async { images_task.await.unwrap() },
        async { ovn_controller_task.await.unwrap() },
        async { ovn_central_task.await.unwrap() },
        async { network_task.await.unwrap() },
        async { router_task.await.unwrap() },
        async { vm_task1.await.unwrap() },
        async { vm_task2.await.unwrap() },
        async { node_task.await.unwrap() },
    )?;

    Err(Error::UnexpectedExit(
        "Cluster controllers should not exit".into(),
    ))
}
