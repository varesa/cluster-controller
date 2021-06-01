mod errors;
mod cluster;
mod host;
#[macro_use]
mod utils;
mod crd;

use std::env;
use kube::Client;
use crate::errors::Error;
use crate::utils::get_version_string;

const NAMESPACE: &str = "cluster-manager";
const GROUP_NAME: &str = "cluster-virt.acl.fi";

#[tokio::main]
async fn main() -> Result<(), Error>{
    let client = Client::try_default().await?;

    let args: Vec<String> = env::args().collect();
    if args.contains(&String::from("--version")) {
        println!("{}", get_version_string());
    } else if args.contains(&String::from("--host")) {
        println!("Starting host-mode");
        host::libvirt::run(client).await?;
    } else {
        println!("Starting cluster-mode");
        cluster::run(client, NAMESPACE).await?;
    }
    Ok(())
}
