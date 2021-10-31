use k8s_openapi::{
    apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition,
};
use kube::{
    api::{
        ListParams,
        Meta,
        WatchEvent,
    },
    Api,
};
use futures::{StreamExt, TryStreamExt};

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
    return Err(Error::Timeout(format!("Apply CRD {}", name)));
}

pub fn name_namespaced<T>(resource: &T) -> String where T: Meta {
    format!(
        "{}-{}",
        Meta::namespace(resource).expect("get resource namespace"),
        Meta::name(resource)
    )
}

#[macro_export]
macro_rules! create_controller {
    ($resource_type:ident, $reconciler:ident, $error_policy:ident, $context:ident) => {
        Controller::new($resource_type, ListParams::default())
            .run($reconciler, $error_policy, $context)
            .for_each(|res| async move {
                match res {
                    Ok(_o) => { /*println!("reconciled {:?}", o)*/ },
                    Err(e) => println!("reconcile failed: {:?}", e),
                }
            })
            .await;
    }
}

pub fn get_version_string() -> String {
    format!("{}-{}", env!("GIT_COUNT"), env!("GIT_HASH"))
}
