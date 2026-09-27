use super::TestResult;
use serde_json::Value;
use std::time::Duration;
use tokio::{
    process::Command,
    sync::{Mutex, MutexGuard},
    time,
};

// /etc/ceph is process-global, and the CI backend has a single OSD. Keep
// concurrent tests from interfering with one another's RBD setup and cleanup.
static CEPH_LOCK: Mutex<()> = Mutex::const_new(());

pub(in crate::integration_tests) struct Ceph {
    _lock: MutexGuard<'static, ()>,
}

impl Ceph {
    pub(in crate::integration_tests) async fn start() -> TestResult<Self> {
        let lock = CEPH_LOCK.lock().await;
        for path in ["/etc/ceph/ceph.conf", "/etc/ceph/ceph.client.admin.keyring"] {
            if !std::path::Path::new(path).is_file() {
                return Err(format!(
                    "Ceph backend not configured: {path}; run ci/ceph-test.sh start first"
                )
                .into());
            }
        }
        let fixture = Self { _lock: lock };
        for pool in ["templates", "volumes"] {
            fixture.command("rbd", &["ls", "--pool", pool]).await?;
        }
        fixture.client_key().await?;
        Ok(fixture)
    }

    async fn command(&self, executable: &str, args: &[&str]) -> TestResult<String> {
        let mut command = Command::new(executable);
        command.args(args).kill_on_drop(true);
        let output = time::timeout(Duration::from_secs(30), command.output())
            .await
            .map_err(|_| format!("{executable} {args:?}: timed out"))??;
        if !output.status.success() {
            return Err(format!(
                "{executable} {args:?}: {}; {}",
                output.status,
                String::from_utf8_lossy(&output.stderr),
            )
            .into());
        }
        Ok(String::from_utf8(output.stdout)?)
    }

    pub(in crate::integration_tests) async fn client_key(&self) -> TestResult<String> {
        Ok(self
            .command("ceph", &["auth", "get-key", "client.libvirt"])
            .await?
            .trim()
            .to_owned())
    }

    pub(in crate::integration_tests) async fn create_image(
        &self,
        pool: &str,
        name: &str,
        size: &str,
    ) -> TestResult {
        self.command(
            "rbd",
            &[
                "create",
                &format!("{pool}/{name}"),
                "--size",
                size,
                "--image-feature",
                "layering",
            ],
        )
        .await?;
        Ok(())
    }

    pub(in crate::integration_tests) async fn create_template(
        &self,
        name: &str,
        size: &str,
    ) -> TestResult {
        self.create_image("templates", name, size).await?;
        let snapshot = format!("templates/{name}@default");
        self.command("rbd", &["snap", "create", &snapshot]).await?;
        self.command("rbd", &["snap", "protect", &snapshot]).await?;
        Ok(())
    }

    pub(in crate::integration_tests) async fn image_exists(
        &self,
        pool: &str,
        name: &str,
    ) -> TestResult<bool> {
        Ok(self
            .command("rbd", &["ls", "--pool", pool])
            .await?
            .lines()
            .any(|image| image == name))
    }

    pub(in crate::integration_tests) async fn image_size(
        &self,
        pool: &str,
        name: &str,
    ) -> TestResult<u64> {
        let info = self
            .command(
                "rbd",
                &["info", "--format", "json", &format!("{pool}/{name}")],
            )
            .await?;
        let info: Value = serde_json::from_str(&info)?;
        info.get("size")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("RBD image size missing: {info}").into())
    }

    pub(in crate::integration_tests) async fn clone_parent(
        &self,
        pool: &str,
        name: &str,
    ) -> TestResult<Option<String>> {
        let info = self
            .command(
                "rbd",
                &["info", "--format", "json", &format!("{pool}/{name}")],
            )
            .await?;
        let info: Value = serde_json::from_str(&info)?;
        let Some(parent) = info.get("parent") else {
            return Ok(None);
        };
        let field = |field| -> TestResult<&str> {
            parent
                .get(field)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("RBD parent missing {field}: {info}").into())
        };
        Ok(Some(format!(
            "{}/{}@{}",
            field("pool")?,
            field("image")?,
            field("snapshot")?
        )))
    }
}
