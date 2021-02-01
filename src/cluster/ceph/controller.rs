use kube::{Client, api::{Api, ListParams, Meta}};
use kube_runtime::controller::{Context, Controller, ReconcilerAction};
use tokio::time::Duration;
use futures::StreamExt;
use humanize_rs::bytes::Bytes;

use crate::errors::Error;
use super::crd::Volume;
use super::lowlevel;

const POOL: &str = "volumes";

async fn reconcile(v: Volume, _ctx: Context<()>) -> Result<ReconcilerAction, Error> {
    let name = format!("{}-{}",
        Meta::namespace(&v).expect("get namespace"), Meta::name(&v)
    );
    let bytes = v.spec.size.parse::<Bytes<u64>>()?.size();
    //println!("{:?}", &v.spec.size);
    //println!("{:?}", bytes);

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
            Some(lowlevel::create_image(pool, name, bytes))
        })
        .transpose()?;

    lowlevel::close_pool(pool);
    lowlevel::disconnect(cluster);

    //println!("{:?}", v);
    Ok(ReconcilerAction {
        requeue_after: Some(Duration::from_secs(600)),
    })
}

fn error_policy(_error: &Error, _ctx: Context<()>) -> ReconcilerAction {
    ReconcilerAction {
        requeue_after: Some(Duration::from_secs(15)),
    }
}

pub async fn create(client: Client) -> Result<(), Error> {
    let context = Context::new(());
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