use crate::cluster::controllers::volumes;
use crate::crd::ceph::{Volume, VolumeSpec};
use crate::integration_tests::harness::{ControlPlane, TestResult, ceph::Ceph};
use crate::{GROUP_NAME, KEYRING_SECRET, NAMESPACE};
use k8s_openapi::api::core::v1::Secret;
use kube::{
    Api,
    api::{Patch, PatchParams, PostParams},
};
use serde_json::json;
use std::{future::Future, time::Duration};
use tokio::{
    task::JoinHandle,
    time::{sleep, timeout},
};
use uuid::Uuid;

struct AbortOnDrop<T>(JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn wait_for<F, Fut>(description: &str, mut condition: F) -> TestResult
where
    F: FnMut() -> Fut,
    Fut: Future<Output = TestResult<bool>>,
{
    timeout(Duration::from_secs(90), async {
        loop {
            if condition().await? {
                return Ok::<(), Box<dyn std::error::Error + Send + Sync>>(());
            }
            sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .map_err(|_| format!("timed out waiting for {description}"))??;
    Ok(())
}

fn has_ceph_finalizer(finalizers: &Option<Vec<String>>) -> bool {
    finalizers.as_ref().is_some_and(|items| {
        items
            .iter()
            .any(|item| item == &format!("{GROUP_NAME}/ceph"))
    })
}

// Exercises the real volume controller against an isolated Kubernetes API and
// the ephemeral CI Ceph monitor/OSD. No RBD operation is simulated by the tests.
struct CephControllerTest {
    _controller: AbortOnDrop<Result<(), crate::errors::Error>>,
    plane: ControlPlane,
    ceph: Ceph,
}

impl CephControllerTest {
    async fn start(test_name: &str) -> TestResult<Self> {
        let ceph = Ceph::start().await?;
        let plane = ControlPlane::start(test_name).await?;
        let controller = AbortOnDrop(tokio::spawn(volumes::create(plane.client())));
        Ok(Self {
            _controller: controller,
            plane,
            ceph,
        })
    }

    fn volumes(&self) -> Api<Volume> {
        Api::namespaced(self.plane.client(), self.plane.namespace())
    }

    fn image_name(&self, volume_name: &str) -> String {
        format!("{}-{volume_name}", self.plane.namespace())
    }
}

fn unique_name(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4().simple())
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn ceph_controller_creates_keyring() -> TestResult {
    let test = CephControllerTest::start("ceph_controller_creates_keyring").await?;
    let secrets: Api<Secret> = Api::namespaced(test.plane.client(), NAMESPACE);

    wait_for("generated libvirt keyring Secret", || async {
        Ok(secrets.get_opt(KEYRING_SECRET).await?.is_some())
    })
    .await?;
    let secret = secrets.get(KEYRING_SECRET).await?;
    let secret_key = secret
        .data
        .as_ref()
        .and_then(|data| data.get("key"))
        .ok_or("Ceph keyring Secret lacks data.key")?;
    assert_eq!(
        serde_json::to_value(secret_key)?,
        serde_json::Value::String(test.ceph.client_key().await?)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn ceph_controller_creates_volume_from_scratch() -> TestResult {
    let test = CephControllerTest::start("ceph_controller_creates_volume_from_scratch").await?;
    let volumes = test.volumes();
    let name = unique_name("scratch");
    let image_name = test.image_name(&name);

    volumes
        .create(
            &PostParams::default(),
            &Volume::new(
                &name,
                VolumeSpec {
                    size: "1 Mi".into(),
                    template: None,
                },
            ),
        )
        .await?;
    wait_for("new volume on RBD", || {
        test.ceph.image_exists("volumes", &image_name)
    })
    .await?;

    assert_eq!(
        test.ceph.image_size("volumes", &image_name).await?,
        1024 * 1024
    );
    assert_eq!(test.ceph.clone_parent("volumes", &image_name).await?, None);
    Ok(())
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn ceph_controller_creates_volume_from_template() -> TestResult {
    let test = CephControllerTest::start("ceph_controller_creates_volume_from_template").await?;
    let volumes = test.volumes();
    let template_name = unique_name("template");
    test.ceph.create_template(&template_name, "1M").await?;
    let name = unique_name("clone");
    let image_name = test.image_name(&name);

    volumes
        .create(
            &PostParams::default(),
            &Volume::new(
                &name,
                VolumeSpec {
                    size: "1 Mi".into(),
                    template: Some(template_name.clone()),
                },
            ),
        )
        .await?;
    wait_for("cloned RBD volume", || {
        test.ceph.image_exists("volumes", &image_name)
    })
    .await?;

    assert_eq!(
        test.ceph.clone_parent("volumes", &image_name).await?,
        Some(format!("templates/{template_name}@default")),
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn ceph_controller_resizes_volume() -> TestResult {
    let test = CephControllerTest::start("ceph_controller_resizes_volume").await?;
    let volumes = test.volumes();
    let name = unique_name("resize");
    let image_name = test.image_name(&name);

    volumes
        .create(
            &PostParams::default(),
            &Volume::new(
                &name,
                VolumeSpec {
                    size: "32 Mi".into(),
                    template: None,
                },
            ),
        )
        .await?;
    wait_for("initial RBD volume", || {
        test.ceph.image_exists("volumes", &image_name)
    })
    .await?;
    volumes
        .patch(
            &name,
            &PatchParams::default(),
            &Patch::Merge(json!({"spec": {"size": "31 Mi"}})),
        )
        .await?;
    sleep(Duration::from_secs(10)).await; // TODO: Show refusal in volume status and wait for that
    assert_eq!(
        test.ceph.image_size("volumes", &image_name).await?,
        32 * 1024 * 1024
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn ceph_controller_refuses_shrinking() -> TestResult {
    let test = CephControllerTest::start("ceph_controller_refuses_shrinking").await?;
    let volumes = test.volumes();
    let name = unique_name("resize");
    let image_name = test.image_name(&name);

    volumes
        .create(
            &PostParams::default(),
            &Volume::new(
                &name,
                VolumeSpec {
                    size: "1 Mi".into(),
                    template: None,
                },
            ),
        )
        .await?;
    wait_for("initial RBD volume", || {
        test.ceph.image_exists("volumes", &image_name)
    })
    .await?;
    volumes
        .patch(
            &name,
            &PatchParams::default(),
            &Patch::Merge(json!({"spec": {"size": "2 Mi"}})),
        )
        .await?;
    wait_for("resized RBD volume", || async {
        Ok(test.ceph.image_size("volumes", &image_name).await? == 2 * 1024 * 1024)
    })
    .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires control plane"]
async fn ceph_controller_deletes_volume() -> TestResult {
    let test = CephControllerTest::start("ceph_controller_deletes_volume").await?;
    let volumes = test.volumes();
    let name = unique_name("delete");
    let image_name = test.image_name(&name);

    volumes
        .create(
            &PostParams::default(),
            &Volume::new(
                &name,
                VolumeSpec {
                    size: "1 Mi".into(),
                    template: None,
                },
            ),
        )
        .await?;
    wait_for("new volume finalizer", || async {
        Ok(has_ceph_finalizer(
            &volumes.get(&name).await?.metadata.finalizers,
        ))
    })
    .await?;
    wait_for("new volume on RBD", || {
        test.ceph.image_exists("volumes", &image_name)
    })
    .await?;

    volumes.delete(&name, &Default::default()).await?;
    wait_for("removed volume and Kubernetes object", || async {
        Ok(volumes.get_opt(&name).await?.is_none()
            && !test.ceph.image_exists("volumes", &image_name).await?)
    })
    .await?;
    Ok(())
}
