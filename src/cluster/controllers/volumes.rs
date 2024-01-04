use humanize_rs::bytes::Bytes;
use k8s_openapi::api::core::v1::Secret;
use kube::runtime::controller::Action;
use kube::{
    api::{Api, Patch, PatchParams},
    error::ErrorResponse,
    Client,
};
use lazy_static::lazy_static;
use serde_json::json;
use std::sync::Arc;
use tokio::time::Duration;
use tracing::{info, instrument};

use crate::crd::ceph::Volume;
use crate::errors::Error;
use crate::shared::ceph::lowlevel;
use crate::utils::extend_traits::ExtendResource;
use crate::utils::resource_controller::{DefaultState, ResourceControllerBuilder};
use crate::utils::strings::field_manager;
use crate::{KEYRING_SECRET, NAMESPACE};

const POOL_VOLUMES: &str = "volumes";
const POOL_TEMPLATES: &str = "templates";
const KEYRING: &str = "client.libvirt";

lazy_static! {
    static ref FIELD_MANAGER: String = field_manager("ceph");
}

/// Check if an volume already exists in the cluster and
/// create if it doesn't.
#[instrument]
fn ensure_exists(name: &str, size: u64, template: Option<String>) -> Result<(), Error> {
    let cluster = lowlevel::connect()?;
    let volume_pool = lowlevel::get_pool(cluster, POOL_VOLUMES.into())?;
    let template_pool = lowlevel::get_pool(cluster, POOL_TEMPLATES.into())?;

    lowlevel::get_images(volume_pool)?
        .iter()
        .find(|&existing| existing == name)
        .map(|_| Ok(()))
        .or_else(|| {
            info!("ceph: Volume {} does not exist", name);
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

#[instrument]
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

#[instrument]
fn get_ceph_keyring() -> Result<String, Error> {
    info!("ceph: Getting keyring from cluster");
    let cluster = lowlevel::connect()?;
    let key = lowlevel::auth_get_key(cluster, KEYRING.into())?;
    lowlevel::disconnect(cluster);

    Ok(key)
}

#[instrument(skip(client))]
async fn create_ceph_secret(client: Client, secret: String) -> Result<(), Error> {
    info!("ceph: Saving keyring in secret");
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
#[instrument(skip(client))]
async fn ensure_keyring(client: Client) -> Result<(), Error> {
    let secrets: Api<Secret> = Api::namespaced(client.clone(), NAMESPACE);
    let keyring = secrets.get(KEYRING_SECRET).await;
    match keyring {
        Ok(_) => {
            info!("ceph: Keyring secret exists");
            Ok(())
        }
        Err(kube::Error::Api(ErrorResponse { code: 404, .. })) => {
            info!("ceph: Keyring missing");
            let key = get_ceph_keyring()?;
            create_ceph_secret(client.clone(), key).await?;
            info!("ceph: Keyring saved");
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

/// Handle updates to volumes in the cluster
async fn update_fn(volume: Arc<Volume>, ctx: Arc<DefaultState>) -> Result<Action, Error> {
    let mut volume = (*volume).clone();
    let name = volume.name_prefixed_with_namespace();
    let bytes = volume.spec.size.parse::<Bytes<u64>>()?.size();
    let template = volume.spec.template.clone();

    info!("ceph: Volume {name} updated");
    volume
        .ensure_finalizer("ceph", ctx.client.clone(), &FIELD_MANAGER)
        .await?;
    ensure_exists(&name, bytes, template)?;
    info!("ceph: Volume {name} update success");

    Ok(Action::requeue(Duration::from_secs(600)))
}

/// Handle updates to volumes in the cluster
async fn remove_fn(volume: Arc<Volume>, ctx: Arc<DefaultState>) -> Result<Action, Error> {
    let mut volume = (*volume).clone();
    let name = volume.name_prefixed_with_namespace();

    info!("ceph: Volume {name} waiting for deletion");
    ensure_removed(&name)?;
    volume
        .remove_finalizer("ceph", ctx.client.clone(), &FIELD_MANAGER)
        .await?;
    info!("ceph: Volume {name} deleted");

    Ok(Action::requeue(Duration::from_secs(600)))
}

#[instrument(skip(client))]
pub async fn create(client: Client) -> Result<(), Error> {
    ensure_keyring(client.clone()).await?;
    info!("ceph: Starting controller");
    ResourceControllerBuilder::new(client)
        .with_default_state()
        .with_default_error_policy()
        .with_functions(update_fn, remove_fn)
        .run()
        .await;
    Ok(())
}
