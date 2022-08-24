use crate::cluster::ovn::logicalswitchport::LogicalSwitchPort;
use crate::cluster::ovn::lowlevel::Ovn;
use kube::Client;
use std::sync::Arc;
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
                let ip = msg.ip;

                let ovn = Arc::new(Ovn::new("10.4.3.1", 6641));
                let ports = LogicalSwitchPort::get_by_ip(ovn, ip.to_string())?;
                println!("backend: Ports for {}: {:#?}", ip, ports);

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
