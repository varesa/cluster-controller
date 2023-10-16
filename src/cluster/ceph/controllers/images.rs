use futures::{StreamExt, TryFuture};
use kube::runtime::controller::{Action, Controller};
use kube::{api::Api, Client};
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use std::future::Future;
use std::hash::Hash;
use std::sync::Arc;
use tokio::time::Duration;

use crate::crd::ceph::Image;
use crate::errors::Error;

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
pub struct ResourceController<ResourceType, State, UpdateFut> {
    client: Client,
    state: Arc<State>,
    error_policy: Box<dyn Fn(Arc<ResourceType>, &Error, Arc<State>) -> Action + Send + Sync>,
    update_fn: Box<dyn Fn(Arc<ResourceType>, Arc<State>) -> UpdateFut>,
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
    pub fn with_functions<UpdateFut>(
        self,
        update_fn: impl Fn(Arc<ResourceType>, Arc<State>) -> UpdateFut + 'static,
    ) -> ResourceController<ResourceType, State, UpdateFut>
    where
        UpdateFut: TryFuture<Ok = Action, Error = crate::Error> + Send + 'static,
        //UpdateFut::Error: std::error::Error + Send + 'static,
    {
        ResourceController {
            client: self.client,
            state: self.state,
            error_policy: self.error_policy,
            update_fn: Box::new(update_fn),
        }
    }
}

impl<ResourceType, State, UpdateFut> ResourceController<ResourceType, State, UpdateFut>
where
    ResourceType: kube::Resource + Clone + Debug + DeserializeOwned + Send + Sync + 'static,
    <ResourceType as kube::Resource>::DynamicType: Clone + Debug + Default + Eq + Hash + Unpin,
    UpdateFut: TryFuture<Ok = Action, Error = Error> + Send + 'static,
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
    }

    async fn reconcile(_image: Arc<ResourceType>, _ctx: Arc<State>) -> Result<Action, Error> {
        Ok(Action::requeue(Duration::from_secs(600)))
    }

    fn error_policy(_object: Arc<ResourceType>, _error: &Error, _ctx: Arc<State>) -> Action {
        Action::requeue(Duration::from_secs(15))
    }
}

async fn update_fn(_image: Arc<Image>, _state: Arc<DefaultState>) -> Result<Action, Error> {
    Ok(Action::requeue(Duration::from_secs(600)))
}

pub async fn create(client: Client) -> Result<(), Error> {
    ResourceControllerBuilder::new(client)
        .with_default_state()
        .with_default_error_policy()
        .with_functions(update_fn)
        .run()
        .await;
    Ok(())
}
