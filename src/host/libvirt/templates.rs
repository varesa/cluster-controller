use askama::Template;

#[derive(Template)]
#[template(path = "domain.xml", escape = "none")]
pub struct DomainTemplate {
    pub name: String,
    pub uuid: String,
    pub cpus: u8,
    pub memory: u32,
    pub memory_unit: String,

    pub network_interfaces: Vec<NetworkInterfaceTemplate>,
    pub storage_devices: Vec<StorageTemplate>,
}

#[derive(Template)]
#[template(path = "network_interface.xml", escape = "none")]
pub struct NetworkInterfaceTemplate {
    pub bridge: String,
    pub mac: String,
}

#[derive(Template)]
#[template(path = "storage.xml", escape = "none")]
pub struct StorageTemplate {
    pub pool: String,
    pub image: String,
    pub device: String,
    pub bootdevice: bool,
    pub bus_slot: u8,
}