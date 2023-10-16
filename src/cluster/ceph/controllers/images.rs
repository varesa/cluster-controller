use futures::{StreamExt, TryFuture};
use kube::runtime::controller::{Action, Controller};
use kube::{api::Api, Client};
use lazy_static::lazy_static;
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use std::future::Future;
use std::hash::Hash;
use std::sync::Arc;
use tokio::time::Duration;

use crate::crd::ceph::Image;
use crate::errors::Error;
use crate::shared::ceph::lowlevel;
use crate::utils::strings::field_manager;

////////

struct DefaultState {
    client: Client,
}

pub struct ResourceControllerBuilder {
    client: Client,
}
pub struct ResourceControllerBuilderWithState<State> {
    client: Client,
    state: Arc<State>,
}
pub struct ResourceControllerBuilderWithStateAndErrorPolicy<ResourceType, State> {
    client: Client,
    state: Arc<State>,
    error_policy: Box<dyn Fn(Arc<ResourceType>, &Error, Arc<State>) -> Action + Send + Sync>,
}
pub struct ResourceController<ResourceType, State, UpdateFut, RemoveFut> {
    client: Client,
    state: Arc<State>,
    error_policy: Box<dyn Fn(Arc<ResourceType>, &Error, Arc<State>) -> Action + Send + Sync>,
    update_fn: Box<dyn Fn(Arc<ResourceType>, Arc<State>) -> UpdateFut>,
    remove_fn: Box<dyn Fn(Arc<ResourceType>, Arc<State>) -> RemoveFut>,
}

impl ResourceControllerBuilder {
    fn new(client: Client) -> ResourceControllerBuilder {
        ResourceControllerBuilder { client }
    }
    fn with_default_state(self) -> ResourceControllerBuilderWithState<DefaultState> {
        let state = Arc::new(DefaultState {
            client: self.client.clone(),
        });
        ResourceControllerBuilderWithState {
            client: self.client,
            state,
        }
    }
}

impl<State> ResourceControllerBuilderWithState<State> {
    fn with_default_error_policy<ResourceType>(
        self,
    ) -> ResourceControllerBuilderWithStateAndErrorPolicy<ResourceType, State>
/*where
        ResourceType: kube::Resource,
        <ResourceType as kube::Resource>::DynamicType: std::default::Default,*/ {
        let error_policy_fn = |_object: Arc<ResourceType>, _error: &Error, _ctx: Arc<State>| {
            Action::requeue(Duration::from_secs(15))
        };

        ResourceControllerBuilderWithStateAndErrorPolicy {
            client: self.client,
            state: self.state,
            error_policy: Box::new(error_policy_fn),
        }
    }
}

impl<ResourceType, State> ResourceControllerBuilderWithStateAndErrorPolicy<ResourceType, State> {
    pub fn with_functions<UpdateFut, RemoveFut>(
        self,
        update_fn: impl Fn(Arc<ResourceType>, Arc<State>) -> UpdateFut + 'static,
        remove_fn: impl Fn(Arc<ResourceType>, Arc<State>) -> RemoveFut + 'static,
    ) -> ResourceController<ResourceType, State, UpdateFut, RemoveFut>
    where
        UpdateFut: TryFuture<Ok = Action, Error = crate::Error> + Send + 'static,
        //UpdateFut::Error: std::error::Error + Send + 'static,
        RemoveFut: TryFuture<Ok = Action, Error = crate::Error> + Send + 'static,
        //RemoveFut::Error: std::error::Error + Send + 'static,
    {
        ResourceController {
            client: self.client,
            state: self.state,
            error_policy: self.error_policy,
            update_fn: Box::new(update_fn),
            remove_fn: Box::new(remove_fn),
        }
    }
}

impl<ResourceType, State, UpdateFut, RemoveFut>
    ResourceController<ResourceType, State, UpdateFut, RemoveFut>
where
    ResourceType: kube::Resource + Clone + Debug + DeserializeOwned + Send + Sync + 'static,
    <ResourceType as kube::Resource>::DynamicType: Clone + Debug + Default + Eq + Hash + Unpin,
    UpdateFut: TryFuture<Ok = Action, Error = Error> + Send + 'static,
    //UpdateFut::Error: std::error::Error + Send + 'static,
    //RemoveFut: TryFuture<Ok = Action, Error = crate::Error> + 'static,
    //RemoveFut::Error: std::error::Error + Send + 'static,
    State: Send + Sync + 'static,
{
    pub fn run(self) -> impl Future {
        async move {
            let api: Api<ResourceType> = Api::all(self.client.clone());
            Controller::new(api, kube::runtime::watcher::Config::default())
                .run(self.update_fn, self.error_policy, self.state)
                .for_each(|res| async move {
                    match res {
                        Ok(_o) => { /*println!("reconciled {:?}", o)*/ }
                        Err(e) => println!("reconcile failed: {:?}", e),
                    }
                })
                .await
        }

        /*let reconcile_fn = |object: Arc<Resource>, state: Arc<State>| {
            let future = if object.meta().deletion_timestamp.is_some() {
                remove_fn(object, state)
            } else {
                update_fn(object, state)
            };
            async { future.await }
        };


        t*/
    }

    async fn reconcile(_image: Arc<ResourceType>, _ctx: Arc<State>) -> Result<Action, Error> {
        Ok(Action::requeue(Duration::from_secs(600)))
    }

    fn error_policy(_object: Arc<ResourceType>, _error: &Error, _ctx: Arc<State>) -> Action {
        Action::requeue(Duration::from_secs(15))
    }
}

////////

const POOL_TEMPLATES: &str = "templates";

lazy_static! {
    static ref FIELD_MANAGER: String = field_manager("ceph");
}

/// State available for the reconcile and error_policy functions
/// called by the Controller
/*struct State {
    client: Client,
}*/

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
            println!(
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
/*
async fn reconcile(image: Arc<Image>, ctx: Arc<DefaultState>) -> Result<Action, Error> {
/
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


fn error_policy(_object: Arc<Image>, _error: &Error, _ctx: Arc<DefaultState>) -> Action {
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
}*/

async fn update_fn(_image: Arc<Image>, _state: Arc<DefaultState>) -> Result<Action, Error> {
    Ok(Action::requeue(Duration::from_secs(600)))
}

async fn remove_fn(_image: Arc<Image>, _state: Arc<DefaultState>) -> Result<Action, Error> {
    Ok(Action::requeue(Duration::from_secs(600)))
}

pub async fn create(client: Client) -> Result<(), Error> {
    ResourceControllerBuilder::new(client)
        .with_default_state()
        .with_default_error_policy()
        .with_functions(update_fn, remove_fn)
        .run()
        .await;
    Ok(())
}
