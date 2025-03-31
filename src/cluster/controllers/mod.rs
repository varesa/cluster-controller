use crate::crd;
use crate::errors::Error;
use futures::try_join;
use kube::Client;

mod images;
mod network;
pub mod node;
mod ovn;
mod router;
mod virtualmachine;
mod volumes;

pub async fn run(client: Client) -> Result<(), Error> {
    crd::libvirt::create(client.clone()).await?;
    crd::ceph::create(client.clone()).await?;
    crd::ovn::create(client.clone()).await?;

    let volumes_task = tokio::spawn(volumes::create(client.clone()));
    let images_task = tokio::spawn(images::create(client.clone()));
    let ovn_task = tokio::spawn(ovn::create(client.clone()));

    let network_task = tokio::spawn(network::create(client.clone()));
    let router_task = tokio::spawn(router::create(client.clone()));

    let vm_task1 = tokio::spawn(virtualmachine::ovn::create(client.clone()));
    let vm_task2 = tokio::spawn(virtualmachine::vm::create(client.clone()));

    let node_task = tokio::spawn(node::create(client.clone()));

    let results = try_join!(
        volumes_task,
        images_task,
        ovn_task,
        network_task,
        router_task,
        vm_task1,
        vm_task2,
        node_task
    )?;
    let (
        result_volumes,
        result_images,
        result_ovn,
        result_network,
        result_router,
        result_vm1,
        result_vm2,
        result_node,
    ) = results;

    result_volumes?;
    result_images?;
    result_ovn?;
    result_network?;
    result_router?;
    result_vm1?;
    result_vm2?;
    result_node?;
    Ok(())
}
