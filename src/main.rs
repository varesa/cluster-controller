use kube::Client;
use std::env;
use tracing::info;

use crate::errors::Error;
use crate::logging::setup_tracing;
use crate::utils::strings::get_version_string;

mod cluster;
mod errors;
mod host;
#[macro_use]
mod utils;
mod crd;
mod logging;
mod metadataservice;
mod shared;

const NAMESPACE: &str = "virt-controller";
const GROUP_NAME: &str = "cluster-virt.acl.fi";
const KEYRING_SECRET: &str = "ceph-client.libvirt";

#[tokio::main]
async fn main() -> Result<(), Error> {
    let args: Vec<String> = env::args().collect();

    if args.contains(&String::from("--version")) {
        // This is used by packaging scripts, ensure no other output gets printed
        println!("{}", get_version_string());
        return Ok(());
    }

    println!("Setting up tracing");
    let _provider = setup_tracing()?;

    info!("Starting up");

    let client = Client::try_default().await?;

    if args.contains(&String::from("--host")) {
        info!("Starting host-mode");
        host::libvirt::run(client).await?;
    } else if args.contains(&String::from("--metadata-service")) {
        info!("Staring metadata service mode");
        metadataservice::run(args, client).await?;
    } else {
        info!("Starting cluster-mode");
        cluster::run(client, NAMESPACE).await?;
    }

    Ok(())
}
