use kube::Client;

use crate::metadataservice::backend::MetadataBackend;
use crate::metadataservice::bidirectional_channel::bidirectional_channel;
use crate::metadataservice::protocol::ChannelProtocol;
use crate::metadataservice::proxy::MetadataProxy;
use crate::Error;

mod backend;
mod bidirectional_channel;
pub mod deployment;
pub mod protocol;
mod proxy;

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

    let (ch_backend, ch_proxy) = bidirectional_channel::<ChannelProtocol>();

    let proxy_task = tokio::task::spawn(async move {
        let proxy_thread = std::thread::spawn(move || MetadataProxy::run(ch_proxy, &router_name));
        panic!(
            "proxy thread exited: {:?}",
            tokio::task::spawn_blocking(|| { proxy_thread.join() }).await
        );
    });

    let backend_task = tokio::task::spawn(async {
        panic!(
            "metadata backend task exited: {:?}",
            MetadataBackend::run(ch_backend, client).await
        );
    });

    println!("supervisor: keeping a watch on the threads");
    let _ = tokio::try_join!(proxy_task, backend_task);

    eprintln!("supervisor: ERROR: One of the threads died, killing the rest of the application");
    std::process::exit(1);
}
