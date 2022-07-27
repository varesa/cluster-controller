use kube::Client;

use crate::crd;
use crate::errors::Error;

mod common;
mod controller;
mod jsonrpc;
mod logicalswitch;
mod lowlevel;
mod types;

pub async fn run(client: Client) -> Result<(), Error> {
    crd::ovn::create(client.clone()).await?;
    controller::create(client.clone()).await?;
    Ok(())
}
