use futures::{StreamExt, TryFuture};
use kube::runtime::controller::{Action, Controller};
use kube::{api::Api, Client};
use std::future::Future;
use std::sync::Arc;
use tokio::time::Duration;

use crate::crd::ceph::Image;
use crate::errors::Error;

// These are simplifications of a module ~200 lines long
pub struct State {}

// A builder that takes in an async callback function from downstream code
pub struct Builder {}

// And crafts a controller which stores the callback function:
pub struct MyControllerWhichDoesNotWork<UpdateFut> {
    update_fn: Box<dyn Fn(Arc<Image>, Arc<State>) -> UpdateFut>,
}

// The real version of this would take multiple functions, but here simplified to a single one:
impl Builder {
    pub fn with_functions<UpdateFut>(
        update_fn: impl Fn(Arc<Image>, Arc<State>) -> UpdateFut + 'static,
    ) -> MyControllerWhichDoesNotWork<UpdateFut>
    // focus point: Here, as far as I understand, I am restricting UpdateFut to,
    // and thus telling the compiler that UpdateFut will be Send
    where
        UpdateFut: TryFuture<Ok = Action, Error = crate::Error> + Send + 'static,
    {
        MyControllerWhichDoesNotWork {
            update_fn: Box::new(update_fn),
        }
    }
}

impl<UpdateFut> MyControllerWhichDoesNotWork<UpdateFut>
// focus point: again applying a trait bound, UpdateFut must be Send
where
    UpdateFut: TryFuture<Ok = Action, Error = Error> + Send + 'static,
{
    pub fn run(self, client: Client) -> impl Future {
        async move {
            let api: Api<Image> = Api::all(client);

            // We are crafting a kube_runtime::Controller, which takes the callback function
            Controller::new(api, kube::runtime::watcher::Config::default())
                .run(
                    self.update_fn,
                    |_: Arc<Image>, _: &Error, _: Arc<State>| Action::await_change(),
                    Arc::new(State {}),
                )
                .for_each(|_| async move {})
                .await
        }
    }
}
