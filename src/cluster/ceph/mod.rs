use kube::Client;

use crate::crd;
use crate::errors::Error;

mod controllers;

pub async fn run(client: Client) -> Result<(), Error> {
    crd::ceph::create(client.clone()).await?;
    controllers::volumes::create(client.clone()).await?;
    controllers::images::create(client.clone()).await?;
    Ok(())
}
