use std::env;

use kube::Client;

use crate::errors::Error;
use crate::utils::get_version_string;

mod cluster;
mod errors;
mod host;
#[macro_use]
mod utils;
mod crd;
mod metadataservice;
mod shared;

const NAMESPACE: &str = "virt-controller";
const GROUP_NAME: &str = "cluster-virt.acl.fi";
const KEYRING_SECRET: &str = "ceph-client.libvirt";

#[tokio::main]
async fn main() -> Result<(), Error> {
    let args: Vec<String> = env::args().collect();

    if args.contains(&String::from("--version")) {
        println!("{}", get_version_string());
        return Ok(());
    }

    let client = Client::try_default().await?;

    if args.contains(&String::from("--host")) {
        println!("Starting host-mode");
        host::libvirt::run(client).await?;
    } else if args.contains(&String::from("--metadata-service")) {
        println!("Staring metadata service mode");
        metadataservice::run(args, client).await?;
    } else {
        println!("Starting cluster-mode");
        cluster::run(client, NAMESPACE).await?;
    }

    Ok(())
}
