use std::time::Duration;

use super::{ControlPlane, TestResult};
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::{Api, Client, Resource, api::ListParams};
use serde::de::DeserializeOwned;
use tokio::time::sleep;

pub(super) async fn install(plane: &mut ControlPlane) -> TestResult {
    let client = plane.client();
    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());

    plane
        .checked("installing Cluster CRD", async {
            crate::crd::cluster::create(client.clone()).await?;
            wait_discoverable::<crate::crd::cluster::Cluster>(
                &crds,
                &client,
                "clusters.cluster-virt.acl.fi",
            )
            .await
        })
        .await?;

    plane
        .checked("installing LibvirtNode CRD", async {
            crate::crd::libvirtnode::create(client.clone()).await?;
            wait_discoverable::<crate::crd::libvirtnode::LibvirtNode>(
                &crds,
                &client,
                "libvirtnodes.cluster-virt.acl.fi",
            )
            .await
        })
        .await?;
    plane
        .checked("installing VirtualMachine CRD", async {
            crate::crd::virtualmachine::create(client.clone()).await?;
            wait_discoverable::<crate::crd::virtualmachine::VirtualMachine>(
                &crds,
                &client,
                "virtualmachines.cluster-virt.acl.fi",
            )
            .await
        })
        .await?;
    plane
        .checked("installing Network CRD", async {
            crate::crd::network::create(client.clone()).await?;
            wait_discoverable::<crate::crd::network::Network>(
                &crds,
                &client,
                "networks.cluster-virt.acl.fi",
            )
            .await
        })
        .await
}

async fn wait_discoverable<K>(
    crds: &Api<CustomResourceDefinition>,
    client: &Client,
    name: &str,
) -> TestResult
where
    K: Resource<DynamicType = ()> + Clone + DeserializeOwned + std::fmt::Debug,
{
    let objects: Api<K> = Api::all(client.clone());
    loop {
        let crd = crds.get(name).await?;
        if crd
            .status
            .and_then(|status| status.conditions)
            .is_some_and(|conditions| {
                conditions
                    .iter()
                    .any(|condition| condition.type_ == "Established" && condition.status == "True")
            })
        {
            // The production helper accepts mere existence; exercise the served endpoint too.
            match objects.list(&ListParams::default()).await {
                Ok(_) => return Ok(()),
                Err(kube::Error::Api(error)) if error.code == 404 || error.code == 503 => {}
                Err(error) => return Err(error.into()),
            }
        }
        sleep(Duration::from_millis(100)).await;
    }
}
