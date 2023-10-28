use kube::Client;

use crate::crd;
use crate::errors::Error;

pub mod common;
mod controllers;
mod deserialization;
mod jsonrpc;
pub mod lowlevel;
pub(crate) mod types;

pub async fn run(client: Client) -> Result<(), Error> {
    crd::ovn::create(client.clone()).await?;
    println!("OVN: CRD created");

    let client_clone = client.clone();
    let network_task = tokio::task::spawn(async {
        panic!(
            "OVN network controller exited: {:?}",
            controllers::network::create(client_clone).await
        );
    });

    let client_clone = client.clone();
    let router_task = tokio::task::spawn(async {
        panic!(
            "OVN router controller exited: {:?}",
            controllers::router::create(client_clone).await
        );
    });

    let client_clone = client.clone();
    let vm_task = tokio::task::spawn(async {
        panic!(
            "OVN vm controller exited: {:?}",
            controllers::vm::create(client_clone).await
        );
    });

    let _ = tokio::try_join!(vm_task, network_task, router_task)?;
    Ok(())
}
