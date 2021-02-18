use sha2::{Sha256, Digest};

const PREFIX: &str = "52:54:00";

pub fn generate_mac_address(seed: String) -> String {
    let mut hasher = Sha256::new();
    hasher.update(seed);
    let hash = hasher.finalize();
    return format!("{}:{:x}:{:x}:{:x}", PREFIX, hash[29], hash[30], hash[31]);
}