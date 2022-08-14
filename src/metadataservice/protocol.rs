use tokio::sync::mpsc::Sender;

#[derive(Debug)]
pub struct MetadataRequest {
    pub ip: String,
    pub return_channel: Sender<MetadataResponse>,
}

#[derive(Debug)]
pub struct MetadataResponse {
    pub ip: String,
    pub metadata: String,
}
