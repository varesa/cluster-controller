use std::{net::SocketAddr, path::Path, process::Command};

use tokio::time;

use super::{POLL_INTERVAL, ServerProcess, TestResult};

pub(super) fn start(
    assets: &Path,
    state: &Path,
    logs: &Path,
    address: SocketAddr,
    peer_address: SocketAddr,
) -> TestResult<ServerProcess> {
    let url = format!("http://{address}");
    let peer_url = format!("http://{peer_address}");
    let mut etcd = Command::new(assets.join("etcd"));
    etcd.env_clear()
        .current_dir(state)
        .arg("--name=control-plane")
        .arg("--data-dir=etcd")
        .arg(format!("--listen-client-urls={url}"))
        .arg(format!("--advertise-client-urls={url}"))
        .arg(format!("--listen-peer-urls={peer_url}"))
        .arg(format!("--initial-advertise-peer-urls={peer_url}"))
        .arg(format!("--initial-cluster=control-plane={peer_url}"));
    ServerProcess::spawn("etcd", &mut etcd, logs)
}

pub(super) async fn ready(address: SocketAddr) -> TestResult {
    loop {
        if tokio::net::TcpStream::connect(address).await.is_ok() {
            return Ok(());
        }
        time::sleep(POLL_INTERVAL).await;
    }
}
