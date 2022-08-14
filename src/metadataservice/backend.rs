use kube::Client;
use tokio::sync::mpsc::Receiver;

use crate::metadataservice::protocol::{MetadataRequest, MetadataResponse};
use crate::Error;

pub struct MetadataBackend {
    channel_endpoint: Receiver<MetadataRequest>,
    // Reserved as part of the interface
    #[allow(dead_code)]
    client: Client,
}

impl MetadataBackend {
    pub async fn run(
        channel_endpoint: Receiver<MetadataRequest>,
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
            if let Some(msg) = self.channel_endpoint.recv().await {
                msg.return_channel
                    .send(MetadataResponse {
                        ip: msg.ip,
                        metadata: format!("Metadata for {}\n", &msg.ip),
                    })
                    .await?;
            }
        }
    }
}
