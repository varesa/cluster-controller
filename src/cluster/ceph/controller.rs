use kube::{Client, api::{Api, ListParams, Meta, Patch}};
use kube_runtime::controller::{Context, Controller, ReconcilerAction};
use tokio::time::Duration;
use futures::StreamExt;
use humanize_rs::bytes::Bytes;
use serde_json::json;
use serde_json::Value as JsonValue;

use crate::GROUP_NAME;
use crate::errors::Error;
use super::crd::Volume;
use super::lowlevel;
use kube::api::PatchParams;
use json_patch::{AddOperation, PatchOperation, patch};

const POOL: &str = "volumes";

struct State {
    client: Client,
}

fn ensure_exists(name: String, size: u64) -> Result<(), Error> {
    let cluster = lowlevel::connect()?;
    let pool = lowlevel::get_pool(cluster, POOL.into())?;

    let images = lowlevel::get_images(pool)?;
    //println!("Images: {:?}", images);

    images
        .iter()
        .find(|&existing| existing == &name)
        .and_then(|existing| {
            println!("Found existing volume: {}", existing);
            Some(Ok(()))
        })
        .or_else(|| {
            println!("Volume {} does not exist", name);
            Some(lowlevel::create_image(pool, name, size))
        })
        .transpose()?;

    lowlevel::close_pool(pool);
    lowlevel::disconnect(cluster);
    Ok(())
}

macro_rules! patch_add {
    ($path:expr, $value:expr) => {
        json_patch::Patch(vec![PatchOperation::Add(AddOperation {
            path: $path,
            value: $value,
        })])
    }
}

async fn ensure_finalizers(client: Client, volume: &Volume) -> Result<(), Error> {
    let volume_name = Meta::name(volume);
    let finalizer_name = format!("{}/ceph", GROUP_NAME);
    let namespace = Meta::namespace(volume).expect("Unable to get namespace");
    let volumes: Api<Volume> = Api::namespaced(client.clone(), &namespace);

    let finalizers = &volume.meta().finalizers;
    if let Some(finalizers) = finalizers {
        for f in finalizers {
            if f == &finalizer_name {
                return Ok(());
            }
        }
    } else {
        volumes.patch(
            &volume_name,
            &PatchParams::default(),
            &Patch::<()>::Json(patch_add!(
                String::from("/metadata/finalizers"),
                JsonValue::Array(vec![JsonValue::String(finalizer_name)])))
        ).await?;

        return Ok(())
    }

    volumes.patch(
        &volume_name,
        &PatchParams::default(),
        &Patch::<()>::Json(patch_add!(
            String::from("/metadata/finalizers/-"),
            JsonValue::String(finalizer_name)))
    ).await?;
    Ok(())
}

async fn reconcile(volume: Volume, ctx: Context<State>) -> Result<ReconcilerAction, Error> {
    let client = ctx.get_ref().client.clone();
    //println!("{:?}", volume);

    let name = format!(
        "{}-{}",
        Meta::namespace(&volume).expect("get namespace"),
        Meta::name(&volume)
    );
    let bytes = volume.spec.size.parse::<Bytes<u64>>()?.size();

    ensure_finalizers(client.clone(), &volume).await?;
    ensure_exists(name, bytes)?;

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
    let context = Context::new(State { client: client.clone() });
    let volumes: Api<Volume> = Api::all(client.clone());
    Controller::new(volumes, ListParams::default())
        .run(reconcile, error_policy, context)
        .for_each(|res| async move {
            match res {
                Ok(_o) => { /*println!("reconciled {:?}", o)*/ },
                Err(e) => println!("reconcile failed: {:?}", e),
            }
        })
        .await;
    Ok(())
}