use crate::crd::libvirt::NetworkAttachment;
use sha2::{Digest, Sha256};

const PREFIX: &str = "52:54:00";

/// Takes a VM name and a network interface specification and generates
/// a MAC address based on a hash of the information
pub fn generate_mac_address(vm_name: &str, nic: &NetworkAttachment, index: usize) -> String {
    let mut hasher = Sha256::new();
    hasher.update(vm_name);
    let network = nic.name.clone();
    let bridge = nic.bridge.clone();
    hasher.update(
        network
            .or(bridge)
            .expect("bridge or network name should be set"),
    );
    hasher.update(vec![index as u8]);
    let hash = hasher.finalize();
    format!("{}:{:x}:{:x}:{:x}", PREFIX, hash[29], hash[30], hash[31])
}
