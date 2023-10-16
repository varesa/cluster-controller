mod errors;
use errors::Error;

use futures::{StreamExt, TryFuture};
use k8s_openapi::api::core::v1::Pod;
use kube::runtime::controller::{Action, Controller};
use kube::{api::Api, Client};
use std::future::Future;
use std::sync::Arc;
use tokio::time::Duration;

async fn update_fn(_: Arc<Pod>, _: Arc<State>) -> Result<Action, Error> {
    Ok(Action::requeue(Duration::from_secs(600)))
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let client = Client::try_default().await?;
    tokio::task::spawn(async {
        Builder::with_functions(update_fn).run(client).await;
    });

    Ok(())
}

// These are simplifications of a module ~200 lines long
pub struct State {}

// A builder that takes in an async callback function from downstream code
pub struct Builder {}

// And crafts a controller which stores the callback function:
pub struct MyControllerWhichDoesNotWork<UpdateFut> {
    update_fn: Box<dyn Fn(Arc<Pod>, Arc<State>) -> UpdateFut>,
}

// The real version of this would take multiple functions, but here simplified to a single one:
impl Builder {
    pub fn with_functions<UpdateFut>(
        update_fn: impl Fn(Arc<Pod>, Arc<State>) -> UpdateFut + 'static,
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
            let api: Api<Pod> = Api::all(client);

            // We are crafting a kube_runtime::Controller, which takes the callback function
            Controller::new(api, kube::runtime::watcher::Config::default())
                .run(
                    self.update_fn,
                    |_: Arc<Pod>, _: &Error, _: Arc<State>| Action::await_change(),
                    Arc::new(State {}),
                )
                .for_each(|_| async move {})
                .await
        }
    }
}

/*

   Compiling cluster-controller v0.1.0 (/home/esav.fi/esa/workspace/cluster-controller)
error[E0277]: `(dyn std::ops::Fn(std::sync::Arc<k8s_openapi::api::core::v1::Pod>, std::sync::Arc<State>) -> impl futures::Future<Output = std::result::Result<kube::kube_runtime::controller::Action, errors::Error>> + 'static)` cannot be sent between threads safely
   --> src/main.rs:19:5
    |
19  |     tokio::task::spawn(async {
    |     ^^^^^^^^^^^^^^^^^^ `(dyn std::ops::Fn(std::sync::Arc<k8s_openapi::api::core::v1::Pod>, std::sync::Arc<State>) -> impl futures::Future<Output = std::result::Result<kube::kube_runtime::controller::Action, errors::Error>> + 'static)` cannot be sent between threads safely
    |
    = help: the trait `std::marker::Send` is not implemented for `(dyn std::ops::Fn(std::sync::Arc<k8s_openapi::api::core::v1::Pod>, std::sync::Arc<State>) -> impl futures::Future<Output = std::result::Result<kube::kube_runtime::controller::Action, errors::Error>> + 'static)`
    = note: required for `Unique<dyn Fn(Arc<Pod>, Arc<State>) -> impl Future<Output = Result<Action, Error>>>` to implement `std::marker::Send`
    = note: the full type name has been written to '/home/esav.fi/esa/workspace/cluster-controller/target/debug/deps/cluster_controller-eeaa1c8bb1dbf652.long-type-15335147078596367422.txt'
note: required because it appears within the type `Box<dyn Fn(Arc<Pod>, Arc<State>) -> impl Future<Output = Result<Action, Error>>>`
   --> /home/esav.fi/esa/.rustup/toolchains/nightly-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/alloc/src/boxed.rs:195:12
    |
195 | pub struct Box<
    |            ^^^
note: required because it's used within this `async` block
   --> src/main.rs:59:9
    |
59  | /         async move {
60  | |             let api: Api<Pod> = Api::all(client);
61  | |
62  | |             // We are crafting a kube_runtime::Controller, which takes the callback function
...   |
70  | |                 .await
71  | |         }
    | |_________^
    = note: required because it captures the following types: `impl futures::Future`
note: required because it's used within this `async` block
   --> src/main.rs:19:24
    |
19  |       tokio::task::spawn(async {
    |  ________________________^
20  | |         Builder::with_functions(update_fn).run(client).await;
21  | |     });
    | |_____^
note: required by a bound in `tokio::spawn`
   --> /home/esav.fi/esa/.cargo/registry/src/index.crates.io-6f17d22bba15001f/tokio-1.29.1/src/task/spawn.rs:166:21
    |
164 |     pub fn spawn<T>(future: T) -> JoinHandle<T::Output>
    |            ----- required by a bound in this function
165 |     where
166 |         T: Future + Send + 'static,
    |                     ^^^^ required by this bound in `spawn`

For more information about this error, try `rustc --explain E0277`.
error: could not compile `cluster-controller` (bin "cluster-controller") due to previous error
[Finished running. Exit status: 101]


*/
