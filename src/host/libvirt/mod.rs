mod controller;
mod handlers;
mod libvirtnode;
mod lowlevel;
mod secrets;
mod templates;
mod utils;

use kube::Client;

use crate::errors::Error;

pub async fn run(client: Client) -> Result<(), Error> {
    controller::create(client.clone()).await?;
    Ok(())
}
