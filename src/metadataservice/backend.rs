use kube::api::ListParams;
use kube::{Api, Client};
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;

use crate::cluster::ovn::common::OvnNamed;
use crate::cluster::ovn::logicalswitchport::LogicalSwitchPort;
use crate::cluster::ovn::lowlevel::Ovn;
use crate::crd::libvirt::v1beta2::VirtualMachine;
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

                let port = if ports.len() == 1 {
                    ports.first().unwrap()
                } else {
                    msg.return_channel
                        .send(MetadataResponse {
                            ip: msg.ip,
                            metadata: None,
                        })
                        .await?;
                    return Err(Error::InstanceMatchFailed(format!(
                        "Matched {} instances",
                        ports.len()
                    )));
                };

                println!("backend: Selected {}", port.name());

                let vms_api: Api<VirtualMachine> = Api::all(self.client.clone());
                let vms = vms_api.list(&ListParams::default()).await?;
                let matching_vms: Vec<&VirtualMachine> = vms
                    .iter()
                    .filter(|vm| {
                        vm.spec
                            .networks
                            .iter()
                            .any(|network| network.ovn_id == Some(port.name()))
                    })
                    .collect();
                assert_eq!(matching_vms.len(), 1);
                let vm = matching_vms.first().unwrap();

                println!("backend: Matched VM {:?}", vm);

                msg.return_channel
                    .send(MetadataResponse {
                        ip: msg.ip,
                        metadata: Some(format!("Metadata for {}\n", &msg.ip)),
                    })
                    .await?;
            }
        }
    }
}
