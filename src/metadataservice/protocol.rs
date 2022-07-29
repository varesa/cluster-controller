pub struct MetadataRequest {
    pub ip: String,
}

pub struct MetadataResponse {
    pub ip: String,
    pub metadata: String,
}

pub enum ChannelProtocol {
    MetadataRequest(MetadataRequest),
    MetadataResponse(MetadataResponse),
}
