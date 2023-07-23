use kube::Client;

use crate::crd;
use crate::errors::Error;

mod controller;
mod scheduling;
mod utils;

pub async fn run(client: Client) -> Result<(), Error> {
    crd::libvirt::create(client.clone()).await?;
    controller::vm::create(client.clone()).await?;
    controller::node::create(client.clone()).await?;
    Ok(())
}
