use crate::crd::cluster::Cluster;
use crate::crd::libvirt::VirtualMachine;
use crate::Error::Volumelocked;
use askama::Template;
use kube::ResourceExt;
use tracing::debug;
use virt::connect::Connect;
use virt::domain::Domain;

use crate::errors::Error;
use crate::host::libvirt::templates::{DomainTemplate, NetworkInterfaceTemplate, StorageTemplate};
use crate::host::libvirt::utils::{get_domain_name, parse_memory};
use crate::shared::ceph;
use crate::utils::extend_traits::TryStatus;

pub struct Libvirt {
    pub connection: Connect,
}

/// virt::connect::Connect does not implement Send due to the raw pointer
/// to the virConnect instance. However according to the library FAQ the
/// library is thread-safe:
/// > Yes, libvirt is thread safe as of version 0.6.0. This means that
/// > multiple threads can act on a single virConnect instance without issue.
unsafe impl Send for Libvirt {}
unsafe impl Sync for Libvirt {}

impl Drop for Libvirt {
    fn drop(&mut self) {
        self.connection.close().expect("close libvirt connection");
    }
}

impl Libvirt {
    pub fn new(uri: &str) -> Result<Self, Error> {
        let connection = Connect::open(uri);
        match connection {
            Ok(connection) => Ok(Self { connection }),
            Err(err) => Err(err.into()),
        }
    }

    pub fn create_domain(&self, vm: &VirtualMachine, cluster: &Cluster) -> Result<(), Error> {
        let namespace = ResourceExt::namespace(vm).expect("VM without namespace?");

        let storage_device_prefix;
        let storage_bus;
        let network_model;
        if vm.spec.compatibility_mode.unwrap_or(false) {
            storage_device_prefix = "sd";
            storage_bus = "sata";
            network_model = "e1000";
        } else {
            storage_device_prefix = "vd";
            storage_bus = "virtio";
            network_model = "virtio";
        }

        let mut volumes = Vec::new();
        for (index, volume) in vm.spec.volumes.iter().enumerate() {
            let drive_index: u8 = index.try_into().expect("Volume index overflows u8");
            volumes.push(StorageTemplate {
                pool: String::from("volumes"),
                image: format!("{}-{}", namespace, volume.name),
                device: format!("{}{}", &storage_device_prefix, (b'a' + drive_index) as char),
                bus_slot: drive_index,
                bootdevice: volumes.is_empty(), // First device is the boot device
                bus: storage_bus.to_string(),
            });
        }

        if volumes_locked(&volumes)? {
            return Err(Volumelocked);
        }

        let mut nics = Vec::new();
        for nic in &vm.try_status()?.networks {
            let bridge = match nic.ovn_id.clone() {
                Some(_) => String::from("br-int"),
                None => nic.bridge.clone().expect("bridge to be set"),
            };
            nics.push(NetworkInterfaceTemplate {
                bridge,
                mac: nic.mac_address.clone().expect("MAC to be set"),
                ovn_id: nic.ovn_id.clone(),
                model: network_model.to_string(),
                queues: nic.queues,
            })
        }
        debug!("{:?}", &vm);
        let (memory_amount, memory_unit) = parse_memory(&vm.spec.memory)?;
        let xml = DomainTemplate {
            name: get_domain_name(vm).expect("no domain name specified"),
            uuid: vm.spec.uuid.clone().expect("VM has no UUID"),
            machine_type: cluster.spec.machine_type.clone(),
            cpu: vm
                .spec
                .cpu_model
                .clone()
                .unwrap_or(cluster.spec.cpu.clone()),
            cpus: vm.spec.cpus,
            memory: memory_amount,
            memory_unit,
            network_interfaces: nics,
            storage_devices: volumes,
        }
        .render()?;

        debug!("{}", xml);
        Domain::create_xml(&self.connection, &xml, 0)?;
        Ok(())
    }

    pub fn has_domain(&self, name: &str) -> Result<bool, Error> {
        let domains = self.connection.list_all_domains(0)?;
        Ok(domains
            .iter()
            .any(|domain| domain.get_name().expect("Failed to get domain name") == name))
    }
}

fn volumes_locked(volumes: &Vec<StorageTemplate>) -> Result<bool, Error> {
    for volume in volumes {
        if ceph::has_locks(&volume.pool, &volume.image)? {
            return Ok(true);
        }
    }
    Ok(false)
}
