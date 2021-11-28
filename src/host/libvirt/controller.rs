use kube::{Client, api::{Api, ListParams, ResourceExt, /*PostParams*/}};
use kube::runtime::controller::{Context, Controller, ReconcilerAction};
use tokio::time::Duration;
use futures::StreamExt;
use std::{convert::TryInto, env};
use virt::{
    domain::Domain,
};
use askama::Template;

//use crate::GROUP_NAME;
use crate::errors::Error;
use crate::crd::libvirt::{VirtualMachine/*,VirtualMachineStatus*/};
use super::lowlevel::Libvirt;
use super::templates::DomainTemplate;
use crate::host::libvirt::templates::{NetworkInterfaceTemplate, StorageTemplate};
use crate::create_controller;
use crate::crd::cluster::Cluster;
use crate::errors::ClusterNotFound;

const LIBVIRT_URI: &str = "qemu:///system";


/// State available for the reconcile and error_policy functions
/// called by the Controller
struct State {
    kube: Client,
    libvirt: Libvirt,
}

/// Construct the expected
fn get_domain_name(vm: &VirtualMachine) -> Option<String> {
    let domain_name: Option<&str> = vm.status.as_ref().map(|status| status.domain_name.as_ref());
    match domain_name {
        Some(name) => Some(String::from(name)),
        _ => {
            let namespace = ResourceExt::namespace(vm)
                .or_else(|| Some(String::from("<no namespace>")))
                .unwrap();
            println!("Ignored VM {}/{} with no domain name defined",
                     namespace, ResourceExt::name(vm));
            None
        }
    }
}

async fn get_cluster(ctx: &Context<State>) -> Result<Cluster, ClusterNotFound> {
    let name: &str = "default";
    let client = ctx.get_ref().kube.clone();
    let clusters: Api<Cluster> = Api::all(client.clone());
    let default = clusters.get(name).await;

    match default {
        Ok(cluster) => Ok(cluster),
        Err(error) => Err(ClusterNotFound {
            name: name.into(),
            inner_error: error,
        })
    }
}

fn create_domain(vm: &VirtualMachine, cluster: &Cluster, ctx: &Context<State>) -> Result<(), Error> {
    let namespace = ResourceExt::namespace(vm).expect("VM without namespace?");
    let mut volumes = Vec::new();
    for (index, volume) in vm.spec.volumes.iter().enumerate() {
        let drive_index: u8 = index.try_into().expect("Volume index overflows u8");
        volumes.push(StorageTemplate {
            pool: String::from("volumes"),
            image: format!("{}-{}", namespace, volume.name),
            device: format!("vd{}", (b'a' + drive_index) as char),
            bus_slot: drive_index,
            bootdevice: volumes.is_empty(), // First device is the boot device
        });
    }
    let mut nics = Vec::new();
    for nic in &vm.spec.networks {
        let bridge = match nic.ovn_id.clone() {
            Some(_) => String::from("br-int"),
            None => nic.bridge.clone().expect("bridge to be set"),
        };
        nics.push(NetworkInterfaceTemplate {
            bridge,
            mac: nic.mac_address.clone().expect("MAC to be set"),
            ovn_id: nic.ovn_id.clone(),
        })
    }
    println!("{:?}", &vm);
    let xml = DomainTemplate {
        name: get_domain_name(vm).expect("no domain name specified"),
        uuid: vm.spec.uuid.clone().expect("VM has no UUID"),
        machine_type: cluster.spec.machine_type.clone(),
        cpu: cluster.spec.cpu.clone(),
        cpus: 1,
        memory: 128,
        memory_unit: String::from("MiB"),
        network_interfaces: nics,
        storage_devices: volumes,
    }.render()?;

    println!("{}", xml);
    Domain::create_xml(&ctx.get_ref().libvirt.connection, &xml, 0)?;
    Ok(())
}

fn refresh_domain(_vm: &VirtualMachine, _domain: &Domain, _ctx: &Context<State>) -> Result<(), Error> {
    Ok(())
}

/// Handle updates to volumes in the cluster
async fn reconcile(vm: VirtualMachine, ctx: Context<State>) -> Result<ReconcilerAction, Error> {
    let my_node_name = env::var("NODE_NAME").expect("failed to read $NODE_NAME");

    println!("Received update to {} on {}", &vm.metadata.name.clone().expect("VM has no name"), my_node_name);

    let vm_name = match get_domain_name(&vm) {
        Some(name) => name,
        None => {
            return Ok(ReconcilerAction { requeue_after: None });
        }
    };

    let target_node = vm.status.as_ref().and_then(|status| status.node.as_ref());
    if let Some(target_node_name) = target_node {
        if target_node_name != &my_node_name {
            println!("Ignored VM {} for another host", vm_name);
            return Ok(ReconcilerAction { requeue_after: None });
        }
    } else {
        println!("Ignored unscheduled VM {}", vm_name);
        return Ok(ReconcilerAction { requeue_after: None });
    }

    // Get cluster capabilities / definition
    let cluster = get_cluster(&ctx).await?;

    println!("Update for VM {} that has been scheduled to us", vm_name);
    let libvirt_domain = Domain::lookup_by_name(&ctx.get_ref().libvirt.connection, &vm_name);
    println!("Domain: {:?}", libvirt_domain);


    match libvirt_domain {
        Ok(domain) => refresh_domain(&vm, &domain, &ctx),
        Err(_) => create_domain(&vm, &cluster, &ctx),
    }?;

    /*let client = ctx.get_ref().client.clone();

    let status = VirtualMachineStatus {
        scheduled: false,
        running: false,
        node: None,
    };
    set_status(&vm, status, client.clone()).await?;*/

    println!("Updated: {}", vm_name);

    Ok(ReconcilerAction {
        requeue_after: Some(Duration::from_secs(600)),
    })
}

fn error_policy(_error: &Error, _ctx: Context<State>) -> ReconcilerAction {
    ReconcilerAction {
        requeue_after: Some(Duration::from_secs(15)),
    }
}

pub async fn create(client: Client) -> Result<(), Error> {
    let context = Context::new(
        State { kube: client.clone(), libvirt: Libvirt::new(LIBVIRT_URI)? });
    let vms: Api<VirtualMachine> = Api::all(client.clone());
    println!("Starting libvirt host controller");
    create_controller!(vms, reconcile, error_policy, context);
    Ok(())
}
