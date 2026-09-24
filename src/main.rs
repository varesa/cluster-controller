use kube::Client;
use std::env;
use tracing::info;

use crate::errors::Error;
use crate::logging::setup_tracing;
use crate::utils::strings::get_version_string;

mod cluster;
mod errors;
mod host;
#[cfg(test)]
mod integration_tests;
#[macro_use]
mod utils;
mod crd;
mod interfaces;
mod labels_and_annotations;
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

    let force_debug = args.len() >= 2 && args[1] == "test-schedule";

    println!("Setting up tracing");
    let _provider = setup_tracing(force_debug)?;

    info!("Starting up");

    let client = Client::try_default().await?;

    if args.len() >= 3 && args[1] == "test-schedule" {
        use crate::cluster::controllers::virtualmachine::scheduling;
        use crate::crd::virtualmachine::VirtualMachine;
        use kube::api::Api;

        let vm_arg = args[2].clone();
        let ignore_affinity = args.contains(&String::from("--ignore-affinity"));

        let (namespace, name) = vm_arg
            .split_once('/')
            .expect("VM argument must be in namespace/name format");

        println!(
            "Running test-schedule for VM: {}/{} (ignore_affinity={})",
            namespace, name, ignore_affinity
        );

        let vms: Api<VirtualMachine> = Api::namespaced(client.clone(), namespace);
        let vm = vms.get(name).await?;

        match scheduling::schedule(&vm, ignore_affinity, client.clone()).await {
            Ok(node) => {
                let node_name = node.metadata.name.unwrap_or_else(|| "<unknown>".into());
                println!("Result: selected node -> {}", node_name);
            }
            Err(e) => {
                eprintln!("Scheduling failed: {}", e);
                return Err(e);
            }
        }
    } else if args.contains(&String::from("--host")) {
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
