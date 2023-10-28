use kube::Client;

use crate::crd;
use crate::errors::Error;

pub mod common;
mod controllers;
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
    controllers::network::create(client.clone()).await?;
    controllers::router::create(client.clone()).await?;
    controllers::vm::create(client.clone()).await?;
    Ok(())
}
