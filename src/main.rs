mod cluster;
mod errors;
mod host;
#[macro_use]
mod utils;
mod crd;
mod shared;

use crate::errors::Error;
use crate::utils::get_version_string;
use kube::Client;
use std::env;

const NAMESPACE: &str = "virt-controller";
const GROUP_NAME: &str = "cluster-virt.acl.fi";
const KEYRING_SECRET: &str = "ceph-client.libvirt";

#[tokio::main]
async fn main() -> Result<(), Error> {
    let args: Vec<String> = env::args().collect();

    if args.contains(&String::from("--version")) {
        println!("{}", get_version_string());
    } else if args.contains(&String::from("--host")) {
        println!("Starting host-mode");
        let client = Client::try_default().await?;
        host::libvirt::run(client).await?;
    } else {
        println!("Starting cluster-mode");
        let client = Client::try_default().await?;
        cluster::run(client, NAMESPACE).await?;
    }
    Ok(())
}
