use crate::errors::Error;
use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::Namespace;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::api::{ListParams, WatchParams};
use kube::core::WatchEvent;
use kube::{Api, Client};
use tracing::{debug, info, instrument};

pub mod resource_controller;
pub mod strings;
#[macro_use]
pub mod shortcuts;
pub mod libvirt_storage;
pub mod traits;

#[instrument]
pub async fn wait_crd_ready(crds: &Api<CustomResourceDefinition>, name: &str) -> Result<(), Error> {
    if crds.get(name).await.is_ok() {
        info!("CRD ok: {}", &name);
        return Ok(());
    }

    let watch_params = WatchParams::default()
        .fields(&format!("metadata.name={name}"))
        .timeout(5);
    let mut stream = crds.watch(&watch_params, "0").await?.boxed();

    while let Some(status) = stream.try_next().await? {
        if let WatchEvent::Modified(crd) = status {
            debug!("Modify event for {}", name);
            if let Some(status) = crd.status {
                if let Some(conditions) = status.conditions {
                    if let Some(pcond) = conditions.iter().find(|c| c.type_ == "NamesAccepted") {
                        if pcond.status == "True" {
                            info!("CRD accepted: {}", name);
                            return Ok(());
                        }
                    }
                }
            }
        }
    }
    Err(Error::Timeout(format!("Apply CRD {name}")))
}

pub async fn get_namespace_names(client: Client) -> Result<Vec<String>, Error> {
    let ns_api: Api<Namespace> = Api::all(client);
    let namespaces = ns_api
        .list(&ListParams::default())
        .await?
        .iter()
        .map(|ns| {
            ns.metadata
                .name
                .as_ref()
                .expect("namespace has no name")
                .to_owned()
        })
        .collect();
    Ok(namespaces)
}
