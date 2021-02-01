mod lowlevel;
mod crd;
mod controller;

use kube::Client;

use crate::errors::Error;

pub async fn run(client: Client) -> Result<(), Error> {
    crd::create(client.clone()).await?;
    controller::create(client.clone()).await?;
    Ok(())
}