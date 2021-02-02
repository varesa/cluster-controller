use kube::{Client, api::{Api, ListParams, Meta, PostParams}};
use kube_runtime::controller::{Context, Controller, ReconcilerAction};
use tokio::time::Duration;
use futures::StreamExt;
use humanize_rs::bytes::Bytes;

use crate::GROUP_NAME;
use crate::errors::Error;
use super::crd::Volume;
use super::lowlevel;

const POOL: &str = "volumes";

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
        .and_then(|_existing| {
            //println!("Found existing volume: {}", existing);
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

/// Ensure that all the volumes have finalizers so that we will be
/// notified in case a volume is marked for deletion from the API
async fn ensure_finalizers(client: Client, volume: &Volume) -> Result<(), Error> {
    let volume_name = Meta::name(volume);
    let finalizer_name = format!("{}/ceph", GROUP_NAME);
    let namespace = Meta::namespace(volume).expect("Unable to get namespace");
    let volumes: Api<Volume> = Api::namespaced(client.clone(), &namespace);

    if let Some(_) = &volume.metadata.finalizers.as_ref().and_then(
        |finalizers| finalizers.iter().find(|&finalizer| finalizer == &finalizer_name)
    ) {
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

/// Handle updates to volumes in the cluster
async fn reconcile(volume: Volume, ctx: Context<State>) -> Result<ReconcilerAction, Error> {
    let name = format!(
        "{}-{}",
        Meta::namespace(&volume).expect("get namespace"),
        Meta::name(&volume)
    );
    let bytes = volume.spec.size.parse::<Bytes<u64>>()?.size();

    ensure_finalizers(ctx.get_ref().client.clone(), &volume).await?;
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