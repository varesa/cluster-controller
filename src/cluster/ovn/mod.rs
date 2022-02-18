mod controller;
mod jsonrpc;
mod lowlevel;
mod types;

use kube::Client;

use crate::crd;
use crate::errors::Error;

pub async fn run(client: Client) -> Result<(), Error> {
    crd::ovn::create(client.clone()).await?;
    controller::create(client.clone()).await?;
    Ok(())
}
