use futures::{StreamExt, TryFuture};
use kube::runtime::controller::{Action, Controller};
use kube::{api::Api, Client};
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use std::future::Future;
use std::hash::Hash;
use std::sync::Arc;
use tokio::time::Duration;
use tracing::{info_span, Instrument};

use crate::errors::Error;

type StoredErrorPolicyFn<ResourceType, State> =
    Box<dyn Fn(Arc<ResourceType>, &Error, Arc<State>) -> Action + Send + Sync>;
type StoredReconcileFn<ResourceType, State, Fut> =
    Box<dyn Fn(Arc<ResourceType>, Arc<State>) -> Fut + Send + Sync>;

pub struct DefaultState {
    pub client: Client,
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
    error_policy: StoredErrorPolicyFn<ResourceType, State>,
}
pub struct ResourceController<ResourceType, State, UpdateFut, RemoveFut> {
    client: Client,
    state: Arc<State>,
    error_policy: StoredErrorPolicyFn<ResourceType, State>,
    update_fn: StoredReconcileFn<ResourceType, State, UpdateFut>,
    remove_fn: StoredReconcileFn<ResourceType, State, RemoveFut>,
}

impl ResourceControllerBuilder {
    pub fn new(client: Client) -> ResourceControllerBuilder {
        ResourceControllerBuilder { client }
    }
    pub fn with_default_state(self) -> ResourceControllerBuilderWithState<DefaultState> {
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
    pub fn with_default_error_policy<ResourceType>(
        self,
    ) -> ResourceControllerBuilderWithStateAndErrorPolicy<ResourceType, State> {
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
        update_fn: impl Fn(Arc<ResourceType>, Arc<State>) -> UpdateFut + Send + Sync + 'static,
        remove_fn: impl Fn(Arc<ResourceType>, Arc<State>) -> RemoveFut + Send + Sync + 'static,
    ) -> ResourceController<ResourceType, State, UpdateFut, RemoveFut>
    where
        UpdateFut: TryFuture<Ok = Action, Error = crate::Error> + Send + 'static,
        RemoveFut: TryFuture<Ok = Action, Error = crate::Error> + Send + 'static,
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
    ResourceType: kube::Resource
        + kube::ResourceExt
        + Clone
        + Debug
        + DeserializeOwned
        + Send
        + Sync
        + 'static,
    <ResourceType as kube::Resource>::DynamicType: Clone + Debug + Default + Eq + Hash + Unpin,
    UpdateFut: Future<Output = Result<Action, Error>> + Send + 'static,
    RemoveFut: Future<Output = Result<Action, Error>> + Send + 'static,
    State: Send + Sync + 'static,
{
    pub fn run(self) -> impl Future {
        let api: Api<ResourceType> = Api::all(self.client.clone());
        let remove_fn = Arc::new(self.remove_fn);
        let update_fn = Arc::new(self.update_fn);

        Controller::new(api, kube::runtime::watcher::Config::default())
            .run(
                move |object: Arc<ResourceType>, state: Arc<State>| {
                    let remove_fn = remove_fn.clone();
                    let update_fn = update_fn.clone();

                    let span = info_span!(
                        "reconcile resource",
                        "kind" =
                            ResourceType::kind(&ResourceType::DynamicType::default()).to_string(),
                        "ns" = object.meta().namespace.clone(),
                        "name" = object.meta().name.clone()
                    );

                    async move {
                        if object.meta().deletion_timestamp.is_some() {
                            remove_fn(object, state)
                                .instrument(info_span!("remove_fn"))
                                .await
                        } else {
                            update_fn(object, state)
                                .instrument(info_span!("update_fn"))
                                .await
                        }
                    }
                    .instrument(span)
                },
                self.error_policy,
                self.state,
            )
            .for_each(|res| async move {
                match res {
                    Ok(_o) => { /*println!("reconciled {:?}", o)*/ }
                    Err(e) => println!("reconcile failed: {:?}", e),
                }
            })
    }
}
