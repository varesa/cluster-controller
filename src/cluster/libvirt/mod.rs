use kube::Client;

use crate::crd;
use crate::errors::Error;

mod controller;
mod scheduling;
mod utils;

pub async fn run(client: Client) -> Result<(), Error> {
    crd::libvirt::create(client.clone()).await?;

    let vm_controller_task = tokio::spawn(controller::vm::create(client.clone()));
    let node_controller_task = tokio::spawn(controller::node::create(client.clone()));

    match tokio::try_join!(vm_controller_task, node_controller_task)? {
        (Err(e), _) => Err(e)?,
        (_, Err(e)) => Err(e)?,
        (Ok(_), Ok(_)) => unreachable!("Forever running tasks should not exit without an error"),
    }
}
