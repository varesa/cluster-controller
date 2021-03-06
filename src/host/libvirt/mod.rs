mod controller;
mod lowlevel;
mod templates;

use kube::Client;

use crate::errors::Error;

pub async fn run(client: Client) -> Result<(), Error> {
    controller::create(client.clone()).await?;
    Ok(())
}