use futures::{SinkExt, StreamExt};
use kube::Client;

use crate::metadataservice::bidirectional_channel::ChannelEndpoint;
use crate::metadataservice::protocol::{ChannelProtocol, MetadataResponse};
use crate::Error;

pub struct MetadataBackend {
    channel_endpoint: ChannelEndpoint<ChannelProtocol>,
    client: Client,
}

impl MetadataBackend {
    pub async fn run(
        channel_endpoint: ChannelEndpoint<ChannelProtocol>,
        client: Client,
    ) -> Result<(), Error> {
        println!("backend: Starting metadata backend");
        let mut mb = MetadataBackend {
            channel_endpoint,
            client,
        };
        mb.main().await
    }

    async fn main(&mut self) -> Result<(), Error> {
        loop {
            if let Some(msg) = self.channel_endpoint.rx.next().await {
                match msg {
                    ChannelProtocol::MetadataRequest(req) => {
                        let ip = req.ip;
                        self.channel_endpoint
                            .tx
                            .send(ChannelProtocol::MetadataResponse(MetadataResponse {
                                ip,
                                metadata: String::from("Hello world"),
                            }))
                            .await?;
                    }
                    _ => panic!("Bad message type"),
                }
            }
        }
    }
}
