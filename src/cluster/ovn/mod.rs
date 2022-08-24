use kube::Client;

use crate::crd;
use crate::errors::Error;

mod common;
mod controller;
mod deserialization;
mod dhcpoptions;
mod jsonrpc;
mod logicalrouter;
mod logicalrouterport;
mod logicalswitch;
pub mod logicalswitchport;
pub mod lowlevel;
mod staticroute;

pub async fn run(client: Client) -> Result<(), Error> {
    crd::ovn::create(client.clone()).await?;
    controller::create(client.clone()).await?;
    Ok(())
}
