mod apiserver;
mod certificates;
mod crds;
mod etcd;

use k8s_openapi::api::core::v1::Namespace;
use kube::{Api, Client, api::PostParams};
use std::{
    env,
    fs::{self, OpenOptions},
    future::Future,
    net::{SocketAddr, TcpListener},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};
use tempfile::TempDir;
use tokio::{sync::Mutex, time};
use uuid::Uuid;

pub(super) type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

const START_TIMEOUT: Duration = Duration::from_secs(60);
const POLL_INTERVAL: Duration = Duration::from_millis(100);

// Hold this until the selected ports are bound, so concurrent harness starts
// cannot select each other's ports between releasing a reservation and spawning.
static START_LOCK: Mutex<()> = Mutex::const_new(());

// Fields drop in declaration order: stop and reap both servers before removing
// their state. This also covers startup errors, cancellation, and unwinding.
pub(super) struct ControlPlane {
    client: Client,
    namespace: String,
    apiserver: Option<ServerProcess>,
    etcd: Option<ServerProcess>,
    state: TempDir,
}

struct ServerProcess {
    name: &'static str,
    child: Child,
}

impl ServerProcess {
    fn spawn(name: &'static str, command: &mut Command, logs: &Path) -> TestResult<Self> {
        let log_path = logs.join(format!("{name}.log"));
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        let child = command
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()
            .map_err(|error| format!("starting {name}: {error}; log: {}", log_path.display()))?;
        Ok(Self { name, child })
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        // kill is harmless if try_wait already reaped an exited child.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl ControlPlane {
    pub(super) async fn start(test_name: &str) -> TestResult<Self> {
        let id = Uuid::new_v4().simple().to_string();
        let label: String = test_name
            .chars()
            .take(64)
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let log_root = env::var_os("CONTROL_PLANE_LOG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| env::temp_dir().join("cluster-controller-control-plane-logs"));
        fs::create_dir_all(&log_root)?;
        let logs = log_root.join(format!("{label}-{id}"));
        fs::create_dir(&logs)?;
        eprintln!("{test_name}: control plane logs: {}", logs.display());

        let result = async {
            let assets = env::var_os("CONTROL_PLANE_BIN")
                .filter(|value| !value.is_empty())
                .ok_or("CONTROL_PLANE_BIN must name the envtest Kubernetes binary directory")?;
            let assets = fs::canonicalize(PathBuf::from(assets))
                .map_err(|error| format!("opening CONTROL_PLANE_BIN directory: {error}"))?;
            for binary in ["etcd", "kube-apiserver"] {
                let path = assets.join(binary);
                if !path.is_file() {
                    return Err(format!(
                        "required control plane binary missing: {}",
                        path.display()
                    )
                    .into());
                }
            }

            let state = tempfile::Builder::new()
                .prefix("cluster-controller-plane-")
                .tempdir()?;
            certificates::generate(state.path())?;
            let namespace = format!("control-plane-{id}");
            let startup_lock = START_LOCK.lock().await;
            let etcd_address = allocate_port();
            let peer_address = allocate_port();
            let api_port = allocate_port().port();
            let client = apiserver::client(state.path(), &namespace, api_port)?;
            let mut plane = Self {
                client,
                namespace,
                apiserver: None,
                etcd: None,
                state,
            };
            plane.etcd = Some(etcd::start(
                &assets,
                plane.state.path(),
                &logs,
                etcd_address,
                peer_address,
            )?);
            plane
                .checked("waiting for etcd to listen", etcd::ready(etcd_address))
                .await?;
            plane.apiserver = Some(apiserver::start(
                &assets,
                plane.state.path(),
                &logs,
                &format!("http://{etcd_address}"),
                api_port,
            )?);
            plane
                .checked(
                    "waiting for authenticated API /readyz",
                    apiserver::ready(plane.client.clone()),
                )
                .await?;
            drop(startup_lock);

            crds::install(&mut plane).await?;
            let client = plane.client();
            let namespace = plane.namespace.clone();
            plane
                .checked("creating test namespace", async {
                    let namespaces: Api<Namespace> = Api::all(client);
                    let mut object = Namespace::default();
                    object.metadata.name = Some(namespace);
                    namespaces.create(&PostParams::default(), &object).await?;
                    Ok(())
                })
                .await?;
            Ok(plane)
        }
        .await;

        result.map_err(|error: Box<dyn std::error::Error + Send + Sync>| {
            format!(
                "{test_name}: {error}; control plane logs: {}",
                logs.display()
            )
            .into()
        })
    }

    pub(super) fn client(&self) -> Client {
        self.client.clone()
    }

    pub(super) fn namespace(&self) -> &str {
        &self.namespace
    }

    async fn checked<T>(
        &mut self,
        stage: &str,
        future: impl Future<Output = TestResult<T>>,
    ) -> TestResult<T> {
        let result = time::timeout(START_TIMEOUT, async {
            tokio::pin!(future);
            let mut ticks = time::interval(POLL_INTERVAL);
            loop {
                tokio::select! {
                    result = &mut future => return result,
                    _ = ticks.tick() => {
                        for process in [&mut self.etcd, &mut self.apiserver].into_iter().flatten() {
                            if let Some(status) = process.child.try_wait()? {
                                return Err(format!("{} exited early: {status}", process.name).into());
                            }
                        }
                    }
                }
            }
        })
        .await
        .map_err(|_| format!("{stage}: timed out after {} seconds", START_TIMEOUT.as_secs()))?;
        result.map_err(|error| format!("{stage}: {error}").into())
    }
}

fn allocate_port() -> SocketAddr {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.local_addr().unwrap()
}
