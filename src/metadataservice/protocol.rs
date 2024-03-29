use crate::Error;
use tokio::sync::mpsc::Sender;

#[derive(Debug)]
pub struct MetadataRequest {
    pub ip: std::net::Ipv4Addr,
    pub return_channel: Sender<MetadataResponse>,
}

#[derive(Debug)]
pub struct MetadataResponse {
    pub ip: std::net::Ipv4Addr,
    // box to break recursive type via Error
    pub metadata: Box<Result<String, Error>>,
}
