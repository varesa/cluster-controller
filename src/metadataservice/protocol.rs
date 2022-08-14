#[derive(Debug)]
pub struct MetadataRequest {
    pub ip: String,
}

#[derive(Debug)]
pub struct MetadataResponse {
    pub ip: String,
    pub metadata: String,
}

#[derive(Debug)]
pub enum ChannelProtocol {
    MetadataRequest(MetadataRequest),
    MetadataResponse(MetadataResponse),
}
