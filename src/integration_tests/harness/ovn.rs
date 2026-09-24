use std::{
    fs::{self, OpenOptions},
    net::{SocketAddr, TcpListener},
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};

use k8s_openapi::api::core::v1::Node;
use kube::{Api, Client, api::PostParams};
use serde_json::Value;
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::{Mutex, MutexGuard},
    time,
};
use uuid::Uuid;

use super::TestResult;
use crate::labels_and_annotations::{OVN_CENTRAL_IP_ANNOTATION, OVN_CENTRAL_MANAGED_LABEL};

const ADDRESS: &str = "127.0.0.1:6641";
const SCHEMA: &str = "/usr/share/ovn/ovn-nb.ovsschema";
const START_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(100);

// Production discovery uses a fixed port. Keep this lock until the process has
// stopped and been reaped, not merely until it starts accepting connections.
static NORTHBOUND_LOCK: Mutex<()> = Mutex::const_new(());

pub(in crate::control_plane_tests) struct OvnNorthbound {
    child: Child,
    _state: TempDir,
    _lock: MutexGuard<'static, ()>,
}

impl OvnNorthbound {
    pub(in crate::control_plane_tests) async fn start(client: Client) -> TestResult<Self> {
        let lock = NORTHBOUND_LOCK.lock().await;
        let state = tempfile::Builder::new()
            .prefix("cluster-controller-ovn-nb-")
            .tempdir()?;
        let db = state.path().join("ovnnb.db");
        let schema = Path::new(SCHEMA);
        if !schema.is_file() {
            return Err(format!("OVN Northbound schema missing: {}", schema.display()).into());
        }
        let created = Command::new("ovsdb-tool")
            .arg("create")
            .arg(&db)
            .arg(schema)
            .output()
            .map_err(|error| format!("running ovsdb-tool create: {error}"))?;
        if !created.status.success() {
            return Err(format!(
                "ovsdb-tool create failed: {}",
                String::from_utf8_lossy(&created.stderr)
            )
            .into());
        }
        // Never mistake an unrelated listener on the fixed production port
        // for the database started by this fixture.
        let reserved = TcpListener::bind(ADDRESS)
            .map_err(|error| format!("OVN Northbound port {ADDRESS} is unavailable: {error}"))?;
        drop(reserved);

        let log_dir = std::env::var_os("CONTROL_PLANE_LOG_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("cluster-controller-control-plane-logs"));
        fs::create_dir_all(&log_dir)?;
        let log_path = log_dir.join(format!("ovn-nb-{}.log", Uuid::new_v4().simple()));
        let log = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&log_path)?;
        let child = Command::new("ovsdb-server")
            .arg("--remote=ptcp:6641:127.0.0.1")
            .arg("--unixctl=none")
            .arg(&db)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()
            .map_err(|error| {
                format!(
                    "starting ovsdb-server: {error}; log: {}",
                    log_path.display()
                )
            })?;
        let mut server = Self {
            child,
            _state: state,
            _lock: lock,
        };

        let ready = time::timeout(START_TIMEOUT, async {
            loop {
                if let Some(status) = server.child.try_wait()? {
                    return Err(format!("ovsdb-server exited early: {status}").into());
                }
                if matches!(
                    time::timeout(Duration::from_secs(1), northbound_ready()).await,
                    Ok(Ok(true))
                ) {
                    return Ok::<(), Box<dyn std::error::Error + Send + Sync>>(());
                }
                time::sleep(POLL_INTERVAL).await;
            }
        })
        .await
        .map_err(|_| {
            format!(
                "OVN Northbound on {ADDRESS} did not become ready in {} seconds",
                START_TIMEOUT.as_secs()
            )
        })?;
        ready.map_err(|error| format!("{error}; ovsdb-server log: {}", log_path.display()))?;

        let mut node = Node::default();
        node.metadata.name = Some(format!("ovn-northbound-{}", Uuid::new_v4().simple()));
        node.metadata.labels = Some([(OVN_CENTRAL_MANAGED_LABEL.into(), "managed".into())].into());
        node.metadata.annotations =
            Some([(OVN_CENTRAL_IP_ANNOTATION.into(), "127.0.0.1".into())].into());
        let nodes: Api<Node> = Api::all(client);
        time::timeout(START_TIMEOUT, nodes.create(&PostParams::default(), &node))
            .await
            .map_err(|_| "timed out registering OVN central Node")??;
        Ok(server)
    }
}

async fn northbound_ready() -> TestResult<bool> {
    let socket: SocketAddr = ADDRESS.parse()?;
    let mut stream = TcpStream::connect(socket).await?;
    stream
        .write_all(b"{\"method\":\"list_dbs\",\"params\":[],\"id\":1}\n")
        .await?;
    // OVSDB replies with one JSON value and does not append a newline.
    let mut response = [0; 4096];
    let mut received = 0;
    let reply: Value = loop {
        if received == response.len() {
            return Err("OVSDB list_dbs response exceeds 4096 bytes".into());
        }
        let bytes = stream.read(&mut response[received..]).await?;
        if bytes == 0 {
            return Err("OVSDB closed before completing list_dbs response".into());
        }
        received += bytes;
        match serde_json::from_slice(&response[..received]) {
            Ok(value) => break value,
            Err(error) if error.is_eof() => continue,
            Err(error) => return Err(error.into()),
        }
    };
    Ok(reply.get("error").is_some_and(Value::is_null)
        && reply.get("id").and_then(Value::as_i64) == Some(1)
        && reply
            .get("result")
            .and_then(Value::as_array)
            .is_some_and(|dbs| dbs.iter().any(|db| db.as_str() == Some("OVN_Northbound"))))
}

impl Drop for OvnNorthbound {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
