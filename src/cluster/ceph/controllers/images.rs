use kube::runtime::controller::Action;
use kube::Client;
use lazy_static::lazy_static;
use std::sync::Arc;
use tokio::time::Duration;
use tracing::{info, instrument};

use crate::crd::ceph::Image;
use crate::errors::Error;
use crate::shared::ceph::lowlevel;
use crate::utils::extend_traits::ExtendResource;
use crate::utils::resource_controller::{DefaultState, ResourceControllerBuilder};
use crate::utils::strings::field_manager;

const POOL_TEMPLATES: &str = "templates";

lazy_static! {
    static ref FIELD_MANAGER: String = field_manager("ceph");
}

/// Check if an image already exists in the cluster and
/// create if it doesn't.
fn ceph_ensure_image_exists(name: &str, source: &str) -> Result<(), Error> {
    let cluster = lowlevel::connect()?;
    let template_pool = lowlevel::get_pool(cluster, POOL_TEMPLATES.into())?;

    lowlevel::get_images(template_pool)?
        .iter()
        .find(|&existing| existing == name)
        .map(|_| Ok(()))
        .or_else(|| {
            info!(
                "ceph: Image {} does not exist, creating from {}",
                name, source
            );
            Some(Err(Error::NotImplemented(
                "image download/copy".to_string(),
            )))
        })
        .unwrap()?;

    lowlevel::close_pool(template_pool);
    lowlevel::disconnect(cluster);
    Ok(())
}

/// Check if the template pool has the named image and delete from the pool if it exists
fn ceph_ensure_image_removed(name: &str) -> Result<(), Error> {
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

async fn update_fn(image: Arc<Image>, ctx: Arc<DefaultState>) -> Result<Action, Error> {
    // To get a mutable copy to allow finalizer addition
    let mut image = (*image).clone();

    let name = image.name_prefixed_with_namespace();
    let source = image.spec.source.clone();

    info!("ceph: Image {name} updated");
    image
        .ensure_finalizer("ceph", ctx.client.clone(), &FIELD_MANAGER)
        .await?;
    ceph_ensure_image_exists(&name, &source)?;
    info!("ceph: Image {name} update success");
    Ok(Action::requeue(Duration::from_secs(600)))
}

async fn remove_fn(image: Arc<Image>, ctx: Arc<DefaultState>) -> Result<Action, Error> {
    // To get a mutable copy to allow finalizer deletion
    let mut image = (*image).clone();

    let name = image.name_prefixed_with_namespace();

    info!("ceph: Image {name} waiting for deletion");
    ceph_ensure_image_removed(&name)?;

    image
        .remove_finalizer("ceph", ctx.client.clone(), &FIELD_MANAGER)
        .await?;
    info!("ceph: Image {name} deleted");

    Ok(Action::requeue(Duration::from_secs(600)))
}

#[instrument(skip(client))]
pub async fn create(client: Client) -> Result<(), Error> {
    ResourceControllerBuilder::new(client)
        .with_default_state()
        .with_default_error_policy()
        .with_functions(update_fn, remove_fn)
        .run()
        .await;
    Ok(())
}
