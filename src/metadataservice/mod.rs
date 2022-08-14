use kube::Client;
use tokio::sync::mpsc::channel;

use crate::metadataservice::backend::MetadataBackend;
use crate::metadataservice::proxy::MetadataProxy;
use crate::Error;

mod backend;
pub mod deployment;
mod networking;
pub mod protocol;
mod proxy;

const REQUEST_CHANNEL_BUFFER_SIZE: usize = 16;

pub async fn run(args: Vec<String>, client: Client) -> Result<(), Error> {
    let mode_index = args
        .iter()
        .position(|arg| arg == "--metadata-service")
        .unwrap();
    let target = args
        .get(mode_index + 1)
        .expect("Target router should follow mode");

    let split: Vec<&str> = target.split('/').collect();
    let namespace = split.get(0).unwrap();
    let router = split.get(1).unwrap();
    let router_name = format!("{}-{}", namespace, router);

    let (request_sender, request_receiver) = channel(REQUEST_CHANNEL_BUFFER_SIZE);

    let proxy_task = tokio::task::spawn(async move {
        let proxy_thread =
            std::thread::spawn(move || MetadataProxy::run(request_sender, &router_name));
        panic!(
            "proxy thread exited: {:?}",
            tokio::task::spawn_blocking(|| { proxy_thread.join() }).await
        );
    });

    let backend_task = tokio::task::spawn(async {
        panic!(
            "metadata backend task exited: {:?}",
            MetadataBackend::run(request_receiver, client).await
        );
    });

    println!("supervisor: keeping a watch on the threads");
    let _ = tokio::try_join!(proxy_task, backend_task);

    eprintln!("supervisor: ERROR: One of the threads died, killing the rest of the application");
    std::process::exit(1);
}
