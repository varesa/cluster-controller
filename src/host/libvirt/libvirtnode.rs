use crate::crd::libvirtnode::{LibvirtNode, LibvirtNodeStatus, set_libvirtnode_status};
use crate::errors::Error;
use crate::host::libvirt::lowlevel::Libvirt;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::PostParams;
use kube::{Api, Client};

pub async fn update(libvirt: &Libvirt, client: Client) -> Result<(), Error> {
    let capabilities = libvirt.connection.get_capabilities()?;
    let libvirt_nodes: Api<LibvirtNode> = Api::all(client.clone());

    let node_name = std::env::var("NODE_NAME").expect("NODE_NAME should be set");

    if let Some(libvirt_node) = libvirt_nodes.get_opt(&node_name).await? {
        let mut status = libvirt_node.status.as_ref().cloned().unwrap_or_default();
        status.capabilities = capabilities;
        set_libvirtnode_status(&libvirt_node, status, client.clone()).await?;
    } else {
        libvirt_nodes
            .create(
                &PostParams::default(),
                &LibvirtNode {
                    metadata: ObjectMeta {
                        name: Some(node_name),
                        ..Default::default()
                    },
                    spec: Default::default(),
                    status: Some(LibvirtNodeStatus { capabilities }),
                },
            )
            .await?;
    };

    Ok(())
}
