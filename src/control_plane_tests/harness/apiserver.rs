use std::{fs, path::Path, process::Command, time::Duration};

use kube::{Client, Config};
use tokio::time;

use super::{POLL_INTERVAL, ServerProcess, TestResult};

pub(super) fn start(
    assets: &Path,
    state: &Path,
    logs: &Path,
    etcd_url: &str,
    port: u16,
) -> TestResult<ServerProcess> {
    let api_url = format!("https://127.0.0.1:{port}");
    let mut apiserver = Command::new(assets.join("kube-apiserver"));
    apiserver
        .env_clear()
        .current_dir(state)
        .arg(format!("--etcd-servers={etcd_url}"))
        .arg("--bind-address=127.0.0.1")
        .arg("--advertise-address=127.0.0.1")
        .arg(format!("--secure-port={port}"))
        .arg("--cert-dir=.")
        .arg("--tls-cert-file=server.crt")
        .arg("--tls-private-key-file=server.key")
        .arg("--client-ca-file=ca.crt")
        .arg("--anonymous-auth=false")
        .arg("--authorization-mode=RBAC")
        .arg("--service-cluster-ip-range=10.0.0.0/24")
        .arg("--disable-admission-plugins=ServiceAccount")
        .arg(format!("--service-account-issuer={api_url}"))
        .arg("--service-account-key-file=service-account.key")
        .arg("--service-account-signing-key-file=service-account.key");
    ServerProcess::spawn("kube-apiserver", &mut apiserver, logs)
}

pub(super) fn client(state: &Path, namespace: &str, port: u16) -> TestResult<Client> {
    let mut config = Config::new(format!("https://127.0.0.1:{port}").parse()?);
    config.default_namespace = namespace.to_owned();
    config.root_cert = Some(vec![fs::read(state.join("ca.der"))?]);
    config.auth_info.client_certificate =
        Some(state.join("admin.crt").to_string_lossy().into_owned());
    config.auth_info.client_key = Some(state.join("admin.key").to_string_lossy().into_owned());
    config.connect_timeout = Some(Duration::from_secs(2));
    config.read_timeout = Some(Duration::from_secs(5));
    config.write_timeout = Some(Duration::from_secs(5));
    Ok(Client::try_from(config)?)
}

pub(super) async fn ready(client: Client) -> TestResult {
    loop {
        let request = kube::core::Request::new("").get("readyz", &Default::default())?;
        if client.request_text(request).await.is_ok() {
            return Ok(());
        }
        time::sleep(POLL_INTERVAL).await;
    }
}
