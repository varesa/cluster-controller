use askama::Template;
use futures::StreamExt;
use k8s_openapi::api::core::v1::Secret;
use kube::runtime::controller::{Context, Controller, ReconcilerAction};
use kube::{
    api::{Api, ListParams, PostParams, ResourceExt},
    Client,
};
use std::{convert::TryInto, env};
use tokio::time::Duration;
use virt::{domain::Domain, secret::Secret as LibvirtSecret};

use super::lowlevel::Libvirt;
use super::templates::{DomainTemplate, SecretTemplate};
use crate::crd::cluster::Cluster;
use crate::crd::libvirt::VirtualMachine;
use crate::errors::ClusterNotFound;
use crate::errors::Error;
use crate::host::libvirt::templates::{NetworkInterfaceTemplate, StorageTemplate};
use crate::{
    api_replace_resource, client_ensure_finalizer, client_remove_finalizer, create_controller,
    ok_and_requeue, ok_no_requeue, resource_has_finalizer, GROUP_NAME, KEYRING_SECRET, NAMESPACE,
};

const LIBVIRT_URI: &str = "qemu:///system";
const CEPH_SECRET_UUID: &str = "8e22b0ac-b429-4ad1-8783-6d792db31349";

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
            println!(
                "Ignored VM {}/{} with no domain name defined",
                namespace,
                ResourceExt::name(vm)
            );
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
        }),
    }
}

fn create_domain(
    vm: &VirtualMachine,
    cluster: &Cluster,
    ctx: &Context<State>,
) -> Result<(), Error> {
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
    }
    .render()?;

    println!("{}", xml);
    Domain::create_xml(&ctx.get_ref().libvirt.connection, &xml, 0)?;
    Ok(())
}

fn refresh_domain(
    _vm: &VirtualMachine,
    _domain: &Domain,
    _ctx: &Context<State>,
) -> Result<(), Error> {
    Ok(())
}

fn is_update_for_us(vm: &VirtualMachine) -> bool {
    let my_node_name = env::var("NODE_NAME").expect("failed to read $NODE_NAME");

    println!(
        "Received update to {} on {}",
        &vm.metadata.name.clone().expect("VM has no name"),
        my_node_name
    );

    let vm_name = match get_domain_name(vm) {
        None => return false,
        Some(name) => name,
    };

    let target_node = vm.status.as_ref().and_then(|status| status.node.as_ref());
    if let Some(target_node_name) = target_node {
        if target_node_name != &my_node_name {
            println!("Ignored VM {} for another host", vm_name);
            return false;
        }
    } else {
        println!("Ignored unscheduled VM {}", vm_name);
        return false;
    }

    true
}

/// Handle updates to volumes in the cluster
async fn reconcile(vm: VirtualMachine, ctx: Context<State>) -> Result<ReconcilerAction, Error> {
    let client = ctx.get_ref().kube.clone();

    if !is_update_for_us(&vm) {
        return ok_no_requeue!();
    }

    let vm_name = get_domain_name(&vm).expect("VM has a libvirt domain name");
    println!("Update for VM {} that has been scheduled to us", vm_name);

    if vm.metadata.deletion_timestamp.is_some() {
        println!("VM {} waiting for deletion by host controller", vm_name);

        match Domain::lookup_by_name(&ctx.get_ref().libvirt.connection, &vm_name) {
            Ok(domain) => {
                println!("Domain {vm_name} exists, destroying");
                domain.destroy()?;
                println!("Domain {vm_name} destroyed");
            }
            Err(_) => {
                println!("Domain {vm_name} doesn't exist, ignoring");
            }
        };

        client_remove_finalizer!(client.clone(), VirtualMachine, &vm, "libvirt-host");

        ok_no_requeue!()
    } else {
        client_ensure_finalizer!(client.clone(), VirtualMachine, &vm, "libvirt-host");

        // Get cluster capabilities / definition
        let cluster = get_cluster(&ctx).await?;

        match Domain::lookup_by_name(&ctx.get_ref().libvirt.connection, &vm_name) {
            Ok(domain) => refresh_domain(&vm, &domain, &ctx),
            Err(_) => create_domain(&vm, &cluster, &ctx),
        }?;

        /*
        let status = VirtualMachineStatus {
            scheduled: false,
            running: false,
            node: None,
        };
        set_status(&vm, status, client.clone()).await?;*/

        println!("Updated: {}", vm_name);

        ok_and_requeue!(600)
    }
}

fn error_policy(_error: &Error, _ctx: Context<State>) -> ReconcilerAction {
    ReconcilerAction {
        requeue_after: Some(Duration::from_secs(15)),
    }
}

fn create_secret(key: &[u8], libvirt: &Libvirt) -> Result<(), Error> {
    let xml = SecretTemplate {
        uuid: CEPH_SECRET_UUID.into(),
        name: "client.libvirt secret".into(),
        usage: "ceph".into(),
    }
    .render()?;

    let secret = LibvirtSecret::define_xml(&libvirt.connection, &xml, 0)?;
    secret.set_value(key, 0)?;

    Ok(())
}

async fn ensure_ceph_secret(kube: Client, libvirt: &Libvirt) -> Result<(), Error> {
    if LibvirtSecret::lookup_by_uuid_string(&libvirt.connection, CEPH_SECRET_UUID).is_ok() {
        println!("Secret found");
        return Ok(());
    }
    println!("Secret missing");

    let secrets: Api<Secret> = Api::namespaced(kube.clone(), NAMESPACE);
    let secret = match secrets.get(KEYRING_SECRET).await {
        Err(e) => return Err(e.into()),
        Ok(secret) => secret,
    };

    let data = secret.data.unwrap();
    let key = data.get("key").unwrap().0.clone();
    create_secret(key.as_ref(), libvirt)?;
    println!("Secret created");
    Ok(())
}

pub async fn create(client: Client) -> Result<(), Error> {
    let libvirt = Libvirt::new(LIBVIRT_URI)?;
    ensure_ceph_secret(client.clone(), &libvirt).await?;
    let context = Context::new(State {
        kube: client.clone(),
        libvirt,
    });
    let vms: Api<VirtualMachine> = Api::all(client.clone());
    println!("Starting libvirt host controller");
    create_controller!(vms, reconcile, error_policy, context);
    Ok(())
}
