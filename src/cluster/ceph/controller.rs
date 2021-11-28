use k8s_openapi::api::core::v1::Secret;
use kube::{Client, api::{Api, ListParams, ResourceExt, Patch, PatchParams, PostParams}, error::ErrorResponse};
use kube::runtime::controller::{Context, Controller, ReconcilerAction};
use tokio::time::Duration;
use futures::StreamExt;
use humanize_rs::bytes::Bytes;
use serde_json::json;

use crate::{GROUP_NAME, NAMESPACE};
use crate::errors::Error;
use crate::crd::ceph::Volume;
use super::lowlevel;
use crate::utils::name_namespaced;
use crate::create_controller;

const POOL: &str = "volumes";
const KEYRING: &str = "client.libvirt";
const KEYRING_SECRET: &str = "ceph-client.libvirt";

/// State available for the reconcile and error_policy functions
/// called by the Controller
struct State {
    client: Client,
}

/// Check if an volume already exists in the cluster and
/// create if it doesn't.
fn ensure_exists(name: String, size: u64) -> Result<(), Error> {
    let cluster = lowlevel::connect()?;
    let pool = lowlevel::get_pool(cluster, POOL.into())?;
    

    lowlevel::get_images(pool)?
        .iter()
        .find(|&existing| existing == &name)
        .map(|_| Ok(()))
        .or_else(|| {
            println!("Volume {} does not exist", name);
            Some(lowlevel::create_image(pool, name, size))
        })
        .transpose()?;

    lowlevel::close_pool(pool);
    lowlevel::disconnect(cluster);
    Ok(())
}

/// Ensure that all the volumes have finalizers so that we will be
/// notified in case a volume is marked for deletion from the API
async fn ensure_finalizers(client: Client, volume: &Volume) -> Result<(), Error> {
    let volume_name = ResourceExt::name(volume);
    let finalizer_name = format!("{}/ceph", GROUP_NAME);
    let namespace = ResourceExt::namespace(volume).expect("Unable to get namespace");
    let volumes: Api<Volume> = Api::namespaced(client.clone(), &namespace);

    if volume.metadata.finalizers.as_ref().and_then(
        |finalizers| finalizers.iter().find(|&finalizer| finalizer == &finalizer_name)
    ).is_some() {
        return Ok(())
    }

    let mut new_vol = volume.to_owned();
    if let Some(finalizers) = new_vol.metadata.finalizers.as_mut() {
        finalizers.push(finalizer_name);
    } else {
        new_vol.metadata.finalizers = Some(vec![finalizer_name]);
    }
    volumes.replace(
        &volume_name,
        &PostParams::default(),
        &new_vol,
    ).await?;
    Ok(())
}

fn get_ceph_keyring() -> Result<String, Error> {
    println!("Ceph: Getting keyring from cluster");
    let cluster = lowlevel::connect()?;
    let key = lowlevel::auth_get_key(cluster, "client.libvirt".into())?;
    lowlevel::disconnect(cluster);

    Ok(key)
}

async fn create_ceph_secret(client: Client, secret: String) -> Result<(), Error> {
    println!("Ceph: Saving keyring in secret");
    let secrets: Api<Secret> = Api::namespaced(client, NAMESPACE);
    println!("?");
    let secret: Secret = serde_json::from_value(json!({
        "apiVersion": "v1",
        "kind": "secret",
        "metadata": {
            "name": KEYRING_SECRET,
            "namespace": NAMESPACE
        },
        "data": {
            "key": secret
        }
    }))?;
    println!("??");
    secrets.patch(KEYRING_SECRET, &PatchParams::apply("ceph-controller-cluster"), &Patch::Apply(&secret)).await?;
    Ok(())
}

/// Ensure that we have a ceph key for libvirt
async fn ensure_keyring(client: Client) -> Result<(), Error> {
    let secrets: Api<Secret> = Api::namespaced(client.clone(), NAMESPACE);
    let keyring = secrets.get(KEYRING).await;
    match keyring {
        Ok(_) => {
            println!("Ceph: Keyring secret exists");
            Ok(())
        },
        Err(kube::Error::Api(ErrorResponse { code: 404, .. })) => {
            println!("Ceph: Keyring missing");
            let key = get_ceph_keyring()?;
            create_ceph_secret(client.clone(), key).await?;
            println!("Ceph: Keyring saved");
            Ok(())
        },
        Err(e) => {
            Err(e.into())
        },
    }

}

/// Handle updates to volumes in the cluster
async fn reconcile(volume: Volume, ctx: Context<State>) -> Result<ReconcilerAction, Error> {
    let name = name_namespaced(&volume);
    let bytes = volume.spec.size.parse::<Bytes<u64>>()?.size();

    if volume.metadata.deletion_timestamp.is_some() {
        println!("Ceph: Volume {} waiting for deletion", &volume.metadata.name.expect("Volume has no name"));
    } else {
        ensure_finalizers(ctx.get_ref().client.clone(), &volume).await?;
        ensure_exists(name, bytes)?;
    }

    Ok(ReconcilerAction {
        requeue_after: Some(Duration::from_secs(600)),
    })
}

fn error_policy(_error: &Error, _ctx: Context<State>) -> ReconcilerAction {
    ReconcilerAction {
        requeue_after: Some(Duration::from_secs(15)),
    }
}

pub async fn create(client: Client) -> Result<(), Error> {
    ensure_keyring(client.clone()).await?;
    let context = Context::new(State { client: client.clone() });
    let volumes: Api<Volume> = Api::all(client.clone());
    println!("Ceph: Starting controller");
    create_controller!(volumes, reconcile, error_policy, context);
    Ok(())
}
