use async_trait::async_trait;
use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::Namespace;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::api::{PostParams, WatchParams};
use kube::core::object::HasStatus;
use kube::{
    api::{ListParams, Resource, WatchEvent},
    Api, Client, CustomResourceExt, ResourceExt,
};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

use crate::errors::Error;
use crate::GROUP_NAME;

pub async fn wait_crd_ready(crds: &Api<CustomResourceDefinition>, name: &str) -> Result<(), Error> {
    if crds.get(name).await.is_ok() {
        println!("CRD ok: {}", &name);
        return Ok(());
    }

    let watch_params = WatchParams::default()
        .fields(&format!("metadata.name={}", name))
        .timeout(5);
    let mut stream = crds.watch(&watch_params, "0").await?.boxed();

    while let Some(status) = stream.try_next().await? {
        if let WatchEvent::Modified(crd) = status {
            println!("Modify event for {}", name);
            if let Some(status) = crd.status {
                if let Some(conditions) = status.conditions {
                    if let Some(pcond) = conditions.iter().find(|c| c.type_ == "NamesAccepted") {
                        if pcond.status == "True" {
                            println!("CRD accepted: {}", name);
                            return Ok(());
                        }
                    }
                }
            }
        }
    }
    Err(Error::Timeout(format!("Apply CRD {}", name)))
}

pub fn name_namespaced<T>(resource: &T) -> String
where
    T: Resource,
{
    format!(
        "{}-{}",
        resource
            .meta()
            .namespace
            .as_ref()
            .expect("get resource namespace"),
        resource.meta().name.as_ref().expect("get resource name")
    )
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

#[macro_export]
macro_rules! create_controller {
    ($resource_type:ident, $reconciler:ident, $error_policy:ident, $context:expr) => {
        Controller::new($resource_type, kube::runtime::watcher::Config::default())
            .run($reconciler, $error_policy, $context)
            .for_each(|res| async move {
                match res {
                    Ok(_o) => { /*println!("reconciled {:?}", o)*/ }
                    Err(e) => println!("reconcile failed: {:?}", e),
                }
            })
            .await
    };
}

#[macro_export]
macro_rules! create_set_status {
    ($resource_type:ident, $resource_status_type:ident, $fn_name:ident) => {
        pub async fn $fn_name(resource: &$resource_type, status: $resource_status_type, client: Client) -> Result<(), Error> {
            let api: Api<$resource_type> = Api::namespaced(
                client.clone(),
                &resource.meta().namespace.as_ref().expect("get resource namespace"),
            );
            let status_update = json!({
                "apiVersion": $resource_type::api_version(&()),
                "kind": $resource_type::kind(&()),
                "metadata": {
                    "name": resource.meta().name.as_ref().expect("get resource name"),
                    "resourceVersion": ResourceExt::resource_version(resource),
                },
                "status": status,
            });
            api
                .replace_status(
                    &resource.metadata.name.as_ref().expect("get resource name"),
                    &PostParams::default(),
                    serde_json::to_vec(&status_update).expect("serialize status"),
                )
                .await?;
            Ok(())
        }
    };
    ($resource_type:ident, $resource_status_type:ident) => {
        create_set_status!($resource_type, $resource_status_type, set_status);
    };
}

#[macro_export]
macro_rules! ok_and_requeue {
    ($duration:expr) => {
        Ok(Action::requeue(Duration::from_secs($duration)))
    };
}

#[macro_export]
macro_rules! ok_no_requeue {
    () => {
        Ok(Action::await_change())
    };
}

pub fn get_version_string() -> String {
    format!("{}-{}", env!("GIT_COUNT"), env!("GIT_HASH"))
}

pub trait TryStatus {
    type Status;
    fn try_status(&self) -> Result<&Self::Status, Error>;
}

impl<T: HasStatus + ResourceExt + CustomResourceExt> TryStatus for T {
    type Status = T::Status;

    fn try_status(&self) -> Result<&Self::Status, Error> {
        self.status().ok_or(Error::NoStatusSubresource(format!(
            "{}/{} in ns {} has no status",
            T::api_resource().kind,
            self.name_any(),
            self.namespace().unwrap_or(String::from("<no namespace>")),
        )))
    }
}

#[async_trait]
pub trait ExtendResource {
    async fn commit(&mut self, client: Client, field_manager: &str) -> Result<(), Error>;
    fn has_finalizer(&self, finalizer_name: &str) -> bool;
    async fn ensure_finalizer(
        &mut self,
        finalizer_name: &str,
        client: Client,
        field_manager: &str,
    ) -> Result<(), Error>;
    async fn remove_finalizer(
        &mut self,
        finalizer_name: &str,
        client: Client,
        field_manager: &str,
    ) -> Result<(), Error>;
}

#[async_trait]
impl<T> ExtendResource for T
where
    for<'a> T: Clone
        + Debug
        + Deserialize<'a>
        + Resource<Scope = k8s_openapi::NamespaceResourceScope>
        + ResourceExt
        + Serialize
        + Sync
        + Send,
    <T as Resource>::DynamicType: Default,
{
    async fn commit(&mut self, client: Client, field_manager: &str) -> Result<(), Error> {
        let api: Api<Self> = if let Some(namespace) = self.namespace() {
            Api::namespaced(client, &namespace)
        } else {
            Api::all(client)
        };

        api.replace(
            &self.name_unchecked(),
            &PostParams {
                dry_run: false,
                field_manager: Some(String::from(field_manager)),
            },
            self,
        )
        .await?;
        Ok(())
    }

    fn has_finalizer(&self, finalizer_name: &str) -> bool {
        self.meta()
            .finalizers
            .as_ref()
            .and_then(|finalizers| {
                finalizers
                    .iter()
                    .find(|&finalizer| finalizer == finalizer_name)
            })
            .is_some()
    }

    async fn ensure_finalizer(
        &mut self,
        finalizer_name: &str,
        client: Client,
        field_manager: &str,
    ) -> Result<(), Error> {
        let finalizer_name = format!("{}/{}", GROUP_NAME, finalizer_name);

        if !self.has_finalizer(&finalizer_name) {
            if let Some(finalizers) = self.meta_mut().finalizers.as_mut() {
                finalizers.push(finalizer_name);
            } else {
                self.meta_mut().finalizers = Some(vec![finalizer_name]);
            }
            self.commit(client, field_manager).await?;
        }
        Ok(())
    }

    async fn remove_finalizer(
        &mut self,
        finalizer_name: &str,
        client: Client,
        field_manager: &str,
    ) -> Result<(), Error> {
        let finalizer_name = format!("{}/{}", GROUP_NAME, finalizer_name);

        if self.has_finalizer(&finalizer_name) {
            self.meta_mut()
                .finalizers
                .as_mut()
                .unwrap()
                .retain(|f| f != &finalizer_name);
            self.commit(client, field_manager).await?;
        }
        Ok(())
    }
}
