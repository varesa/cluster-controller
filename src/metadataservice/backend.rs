use k8s_openapi::api::core::v1::ConfigMap;
use kube::api::ListParams;
use kube::{Api, Client, Resource};
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
                            metadata: Box::new(Err(Error::InstanceMatchFailed(format!(
                                "Matched {} instances",
                                ports.len()
                            )))),
                        })
                        .await?;
                    continue;
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

                if let Some(userdata_name) = &vm.spec.userdata {
                    let cm_api: Api<ConfigMap> =
                        Api::namespaced(self.client.clone(), vm.meta().namespace.as_ref().unwrap());

                    let maybe_userdata = cm_api
                        .get(userdata_name)
                        .await
                        .as_ref()
                        .map_err(|e| {
                            println!("backend: {:?}", e);
                            Error::ConfigMapNotFound(userdata_name.to_string())
                        })
                        .and_then(|config_map| {
                            config_map.data.as_ref().ok_or_else(|| {
                                Error::ConfigMapInvalid(
                                    userdata_name.to_string(),
                                    String::from("no .data"),
                                )
                            })
                        })
                        .and_then(|cm_data| {
                            cm_data.get("userdata").ok_or_else(|| {
                                Error::ConfigMapInvalid(
                                    userdata_name.to_string(),
                                    String::from("no .data.userdata"),
                                )
                            })
                        })
                        .map(|userdata_ref| userdata_ref.clone());

                    match maybe_userdata {
                        Ok(userdata) => {
                            msg.return_channel
                                .send(MetadataResponse {
                                    ip: msg.ip,
                                    metadata: Box::new(Ok(userdata)),
                                })
                                .await?;
                        }
                        Err(e) => {
                            println!("backend: error fetching metadata for {}: {:?}", &msg.ip, e);
                            msg.return_channel
                                .send(MetadataResponse {
                                    ip: msg.ip,
                                    metadata: Box::new(Err(e)),
                                })
                                .await?;
                        }
                    }
                } else {
                    println!("backend: No userdata specified in vm spec");
                    msg.return_channel
                        .send(MetadataResponse {
                            ip: msg.ip,
                            metadata: Box::new(Err(Error::UserdataNotSpecified)),
                        })
                        .await?;
                }
            }
        }
    }
}
