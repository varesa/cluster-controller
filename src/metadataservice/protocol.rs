use crate::Error;
use tokio::sync::mpsc::Sender;

#[derive(Debug)]
pub struct MetadataRequest {
    pub ip: std::net::Ipv4Addr,
    pub return_channel: Sender<MetadataResponse>,
}

#[derive(Debug)]
pub struct MetadataResponse {
    // box to break recursive type via Error
    pub metadata: Box<Result<MetadataPayload, Error>>,
}

#[derive(Debug)]
pub struct MetadataPayload {
    pub ip: std::net::Ipv4Addr,
    pub instance_id: String,
    pub hostname: String,
    pub user_data: String,
}
