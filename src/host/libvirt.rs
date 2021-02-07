use kube::{Client};
use crate::errors::Error;
use virt::connect::Connect;

const LIBVIRT_URI: &str = "qemu:///system";

pub fn run(_client: Client) -> Result<(), Error> {
    let connection = Connect::open(LIBVIRT_URI)?;
    println!("Domains: {}", connection.num_of_defined_domains().expect("get domains"));
    Ok(())
}