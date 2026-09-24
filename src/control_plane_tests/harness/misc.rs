use crate::control_plane_tests::harness::TestResult;
use std::fs::OpenOptions;
use std::net::{SocketAddr, TcpListener};
use std::path::Path;
use std::process::{Child, Command, Stdio};

pub(crate) struct ServerProcess {
    pub(crate) name: &'static str,
    pub(crate) child: Child,
}

impl ServerProcess {
    pub(crate) fn spawn(
        name: &'static str,
        command: &mut Command,
        logs: &Path,
    ) -> TestResult<Self> {
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

pub(crate) fn allocate_port() -> SocketAddr {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.local_addr().unwrap()
}
