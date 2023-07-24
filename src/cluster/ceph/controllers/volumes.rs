use futures::StreamExt;
use humanize_rs::bytes::Bytes;
use k8s_openapi::api::core::v1::Secret;
use kube::runtime::controller::{Action, Controller};
use kube::{
    api::{Api, ListParams, Patch, PatchParams},
    error::ErrorResponse,
    Client,
};
use serde_json::json;
use std::sync::Arc;
use tokio::time::Duration;

use crate::crd::ceph::Volume;
use crate::create_controller;
use crate::errors::Error;
use crate::shared::ceph::lowlevel;
use crate::utils::name_namespaced;
use crate::utils::ExtendResource;
use crate::{KEYRING_SECRET, NAMESPACE};

const POOL_VOLUMES: &str = "volumes";
const POOL_TEMPLATES: &str = "templates";
const KEYRING: &str = "client.libvirt";

/// State available for the reconcile and error_policy functions
/// called by the Controller
struct State {
    client: Client,
}

/// Check if an volume already exists in the cluster and
/// create if it doesn't.
fn ensure_exists(name: &str, size: u64, template: Option<String>) -> Result<(), Error> {
    let cluster = lowlevel::connect()?;
    let volume_pool = lowlevel::get_pool(cluster, POOL_VOLUMES.into())?;
    let template_pool = lowlevel::get_pool(cluster, POOL_TEMPLATES.into())?;

    lowlevel::get_images(volume_pool)?
        .iter()
        .find(|&existing| existing == name)
        .map(|_| Ok(()))
        .or_else(|| {
            println!("ceph: Volume {} does not exist", name);
            if let Some(template_name) = template {
                Some(lowlevel::clone_image(
                    volume_pool,
                    name,
                    size,
                    template_pool,
                    &template_name,
                ))
            } else {
                Some(lowlevel::create_image(volume_pool, name, size))
            }
        })
        .unwrap()?;

    lowlevel::close_pool(volume_pool);
    lowlevel::close_pool(template_pool);
    lowlevel::disconnect(cluster);
    Ok(())
}

fn ensure_removed(name: &str) -> Result<(), Error> {
    let cluster = lowlevel::connect()?;
    let pool = lowlevel::get_pool(cluster, POOL_VOLUMES.into())?;

    if lowlevel::get_images(pool)?
        .iter()
        .any(|existing_name| existing_name == name)
    {
        lowlevel::remove_image(pool, name)?;
    }
    Ok(())
}

fn get_ceph_keyring() -> Result<String, Error> {
    println!("ceph: Getting keyring from cluster");
    let cluster = lowlevel::connect()?;
    let key = lowlevel::auth_get_key(cluster, KEYRING.into())?;
    lowlevel::disconnect(cluster);

    Ok(key)
}

async fn create_ceph_secret(client: Client, secret: String) -> Result<(), Error> {
    println!("ceph: Saving keyring in secret");
    let secrets: Api<Secret> = Api::namespaced(client, NAMESPACE);
    let secret: Secret = serde_json::from_value(json!({
        "apiVersion": "v1",
        "kind": "Secret",
        "metadata": {
            "name": KEYRING_SECRET,
            "namespace": NAMESPACE
        },
        "data": {
            "key": secret
        }
    }))?;
    secrets
        .patch(
            KEYRING_SECRET,
            &PatchParams::apply("ceph-controller-cluster"),
            &Patch::Apply(&secret),
        )
        .await?;
    Ok(())
}

/// Ensure that we have a ceph key for libvirt
async fn ensure_keyring(client: Client) -> Result<(), Error> {
    let secrets: Api<Secret> = Api::namespaced(client.clone(), NAMESPACE);
    let keyring = secrets.get(KEYRING_SECRET).await;
    match keyring {
        Ok(_) => {
            println!("ceph: Keyring secret exists");
            Ok(())
        }
        Err(kube::Error::Api(ErrorResponse { code: 404, .. })) => {
            println!("ceph: Keyring missing");
            let key = get_ceph_keyring()?;
            create_ceph_secret(client.clone(), key).await?;
            println!("ceph: Keyring saved");
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

/// Handle updates to volumes in the cluster
async fn reconcile(volume: Arc<Volume>, ctx: Arc<State>) -> Result<Action, Error> {
    let mut volume = (*volume).clone();
    let name = name_namespaced(&volume);
    let bytes = volume.spec.size.parse::<Bytes<u64>>()?.size();
    let template = volume.spec.template.clone();

    if volume.metadata.deletion_timestamp.is_some() {
        println!("ceph: Volume {name} waiting for deletion");
        ensure_removed(&name)?;
        volume
            .remove_finalizer("ceph", ctx.client.clone(), "cluster-controller.ceph")
            .await?;
        println!("ceph: Volume {name} deleted");
    } else {
        println!("ceph: Volume {name} updated");
        volume
            .ensure_finalizer("ceph", ctx.client.clone(), "cluster-controller.ceph")
            .await?;
        ensure_exists(&name, bytes, template)?;
        println!("ceph: Volume {name} update success");
    }

    Ok(Action::requeue(Duration::from_secs(600)))
}

fn error_policy(_error: &Error, _ctx: Arc<State>) -> Action {
    Action::requeue(Duration::from_secs(15))
}

pub async fn create(client: Client) -> Result<(), Error> {
    ensure_keyring(client.clone()).await?;
    let context = Arc::new(State {
        client: client.clone(),
    });
    let volumes: Api<Volume> = Api::all(client.clone());
    println!("ceph: Starting controller");
    create_controller!(volumes, reconcile, error_policy, context);
    Ok(())
}
