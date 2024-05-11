use askama::Template;

#[derive(Template)]
#[template(path = "domain.xml", escape = "none")]
pub struct DomainTemplate {
    pub name: String,
    pub uuid: String,

    pub machine_type: String,
    pub cpu: String,

    pub cpus: usize,
    pub memory: usize,
    pub memory_unit: String,

    pub network_interfaces: Vec<NetworkInterfaceTemplate>,
    pub storage_devices: Vec<StorageTemplate>,
}

#[derive(Template)]
#[template(path = "network_interface.xml", escape = "none")]
pub struct NetworkInterfaceTemplate {
    pub bridge: String,
    pub mac: String,
    pub ovn_id: Option<String>,
    pub model: String,
    pub queues: u8,
}

pub struct CephSource {
    pub pool: String,
    pub image: String,
}

pub struct FilesystemSource {
    pub location: String,
}

pub enum StorageSource {
    Ceph(CephSource),
    Filesystem(FilesystemSource),
}

#[derive(Template)]
#[template(path = "storage.xml", escape = "none")]
pub struct StorageTemplate {
    pub source: StorageSource,
    pub device: String,
    pub bootdevice: bool,
    pub bus_slot: u8,
    pub bus: String,
}

#[derive(Template)]
#[template(path = "secret.xml", escape = "none")]
pub struct SecretTemplate {
    pub uuid: String,
    pub name: String,
    pub usage: String,
}
