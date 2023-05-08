use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::Namespace;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::core::object::HasStatus;
use kube::{
    api::{ListParams, Resource, WatchEvent},
    Api, Client, CustomResourceExt, ResourceExt,
};

use crate::errors::Error;

pub async fn wait_crd_ready(crds: &Api<CustomResourceDefinition>, name: &str) -> Result<(), Error> {
    if crds.get(name).await.is_ok() {
        println!("CRD ok: {}", &name);
        return Ok(());
    }

    let list_params = ListParams::default()
        .fields(&format!("metdata.name={}", name))
        .timeout(5);
    let mut stream = crds.watch(&list_params, "0").await?.boxed();

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
        Controller::new($resource_type, ListParams::default())
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
macro_rules! api_replace_resource {
    ($api:expr, $resource:expr) => {
        $api.replace(
            &$resource.metadata.name.as_ref().expect("get resource name"),
            &PostParams::default(),
            $resource,
        )
        .await?;
    };
}

#[macro_export]
macro_rules! client_replace_resource {
    ($client:ident, $resource_type:ident, $resource:ident) => {
        let api: Api<$resource_type> = Api::namespaced(
            $client.clone(),
            &$resource
                .metadata
                .namespace
                .as_ref()
                .expect("get resource namespace"),
        );
        api_replace_resource!(api, $resource);
    };
}

#[macro_export]
macro_rules! resource_has_finalizer {
    ($resource:expr, $finalizer_name:expr) => {
        $resource
            .metadata
            .finalizers
            .as_ref()
            .and_then(|finalizers| {
                finalizers
                    .iter()
                    .find(|&finalizer| finalizer == $finalizer_name)
            })
            .is_some()
    };
}

#[macro_export]
macro_rules! client_ensure_finalizer {
    ($client:expr, $resource_type:ident, $resource:expr, $controller_name:expr) => {
        let finalizer_name = format!("{}/{}", GROUP_NAME, $controller_name);
        let namespace = $resource
            .metadata
            .namespace
            .as_ref()
            .expect("Unable to get namespace");
        let api: Api<$resource_type> = Api::namespaced($client.clone(), &namespace);

        #[allow(clippy::needless_borrow)]
        if !resource_has_finalizer!($resource, &finalizer_name) {
            let mut new_resource = $resource.to_owned();
            if let Some(finalizers) = new_resource.metadata.finalizers.as_mut() {
                finalizers.push(finalizer_name);
            } else {
                new_resource.metadata.finalizers = Some(vec![finalizer_name]);
            }
            api_replace_resource!(api, &new_resource);
        }
    };
}

#[macro_export]
macro_rules! client_remove_finalizer {
    ($client:expr, $resource_type:ident, $resource:expr, $controller_name:expr) => {
        let finalizer_name = format!("{}/{}", GROUP_NAME, $controller_name);
        let namespace = $resource
            .metadata
            .namespace
            .as_ref()
            .expect("Unable to get namespace");
        let api: Api<$resource_type> = Api::namespaced($client.clone(), &namespace);

        #[allow(clippy::needless_borrow)]
        if resource_has_finalizer!($resource, &finalizer_name) {
            let mut finalizers = $resource.metadata.finalizers.clone().unwrap();
            finalizers.retain(|f| f != &finalizer_name);
            let mut new_resource = $resource.to_owned();
            new_resource.metadata.finalizers = Some(finalizers);
            api_replace_resource!(api, &new_resource);
        }
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
