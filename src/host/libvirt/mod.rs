mod controller;
mod lowlevel;

use kube::Client;

use crate::crd;
use crate::errors::Error;

pub async fn run(client: Client) -> Result<(), Error> {
    controller::create(client.clone()).await?;
    Ok(())
}