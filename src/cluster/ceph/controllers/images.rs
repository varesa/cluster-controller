use futures::StreamExt;
use kube::runtime::controller::{Action, Controller};
use kube::{
    api::Api,
    Client,
};
use lazy_static::lazy_static;
use std::sync::Arc;
use tokio::time::Duration;

use crate::crd::ceph::Image;
use crate::create_controller;
use crate::errors::Error;
use crate::shared::ceph::lowlevel;
use crate::utils::extend_traits::ExtendResource;
use crate::utils::strings::field_manager;

const POOL_TEMPLATES: &str = "templates";

lazy_static! {
    static ref FIELD_MANAGER: String = field_manager("ceph");
}

/// State available for the reconcile and error_policy functions
/// called by the Controller
struct State {
    client: Client,
}

/// Check if an image already exists in the cluster and
/// create if it doesn't.
fn ensure_exists(name: &str, source: &str) -> Result<(), Error> {
    let cluster = lowlevel::connect()?;
    let template_pool = lowlevel::get_pool(cluster, POOL_TEMPLATES.into())?;

    lowlevel::get_images(template_pool)?
        .iter()
        .find(|&existing| existing == name)
        .map(|_| Ok(()))
        .or_else(|| {
            println!("ceph: Image {} does not exist, creating from {}", name, source);
            Some(Err(Error::NotImplemented("image download/copy".to_string())))
        })
        .unwrap()?;

    lowlevel::close_pool(template_pool);
    lowlevel::disconnect(cluster);
    Ok(())
}

/// Check if the template pool has the named image and delete from the pool if it exists
fn ensure_removed(name: &str) -> Result<(), Error> {
    let cluster = lowlevel::connect()?;
    let pool = lowlevel::get_pool(cluster, POOL_TEMPLATES.into())?;

    if lowlevel::get_images(pool)?
        .iter()
        .any(|existing_name| existing_name == name)
    {
        lowlevel::remove_image(pool, name)?;
    }
    Ok(())
}

/// Handle updates to images in the cluster
async fn reconcile(image: Arc<Image>, ctx: Arc<State>) -> Result<Action, Error> {
    let mut image = (*image).clone();
    let name = image.name_prefixed_with_namespace();
    let source = image.spec.source.clone();

    if image.metadata.deletion_timestamp.is_some() {
        println!("ceph: Image {name} waiting for deletion");
        ensure_removed(&name)?;
        image
            .remove_finalizer("ceph", ctx.client.clone(), &FIELD_MANAGER)
            .await?;
        println!("ceph: Image {name} deleted");
    } else {
        println!("ceph: Image {name} updated");
        image
            .ensure_finalizer("ceph", ctx.client.clone(), &FIELD_MANAGER)
            .await?;
        ensure_exists(&name, &source)?;
        println!("ceph: Image {name} update success");
    }

    Ok(Action::requeue(Duration::from_secs(600)))
}

fn error_policy(_object: Arc<Image>, _error: &Error, _ctx: Arc<State>) -> Action {
    Action::requeue(Duration::from_secs(15))
}

pub async fn create(client: Client) -> Result<(), Error> {
    let context = Arc::new(State {
        client: client.clone(),
    });
    let images: Api<Image> = Api::all(client.clone());
    println!("ceph: Starting controller");
    create_controller!(images, reconcile, error_policy, context);
    Ok(())
}
