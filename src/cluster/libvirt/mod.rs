use kube::Client;

use crate::crd::libvirt;
use crate::errors::Error;

mod controller;

pub async fn run(client: Client) -> Result<(), Error> {
    libvirt::create(client.clone()).await?;
    controller::create(client.clone()).await?;
    Ok(())
}