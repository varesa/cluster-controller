use crate::cluster::controllers::{images, volumes};
use crate::crd::ceph::{Image, ImageSpec, Volume, VolumeSpec};
use crate::integration_tests::harness::{ControlPlane, TestResult, ceph::Ceph};
use crate::{GROUP_NAME, KEYRING_SECRET, NAMESPACE};
use k8s_openapi::api::core::v1::Secret;
use kube::{Api, api::PostParams};
use std::{future::Future, time::Duration};
use tokio::{
    task::JoinHandle,
    time::{sleep, timeout},
};

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

// Exercises real controllers against an isolated Kubernetes API and the
// ephemeral CI Ceph monitor/OSD. No RBD operation is simulated by the test.
#[tokio::test]
#[ignore = "requires control plane"]
async fn ceph_controllers_reconcile_keyring_volumes_clones_and_images() -> TestResult {
    let ceph = Ceph::start().await?;
    let plane =
        ControlPlane::start("ceph_controllers_reconcile_keyring_volumes_clones_and_images").await?;
    let client = plane.client();

    let secrets: Api<Secret> = Api::namespaced(client.clone(), NAMESPACE);
    let mut volume_controller = AbortOnDrop(tokio::spawn(volumes::create(client.clone())));
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
        serde_json::Value::String(ceph.client_key().await?)
    );
    let secret_data = secret.data.clone();
    let secret_version = secret.metadata.resource_version.clone();

    let volumes: Api<Volume> = Api::namespaced(client.clone(), plane.namespace());
    let scratch_name = format!("{}-scratch", plane.namespace());
    volumes
        .create(
            &PostParams::default(),
            &Volume::new(
                "scratch",
                VolumeSpec {
                    size: "1 Mi".into(),
                    template: None,
                },
            ),
        )
        .await?;
    wait_for("new volume on RBD", || {
        ceph.image_exists("volumes", &scratch_name)
    })
    .await?;
    wait_for("new volume finalizer", || async {
        Ok(has_ceph_finalizer(
            &volumes.get("scratch").await?.metadata.finalizers,
        ))
    })
    .await?;
    volumes.delete("scratch", &Default::default()).await?;
    wait_for("removed volume and Kubernetes object", || async {
        Ok(volumes.get_opt("scratch").await?.is_none()
            && !ceph.image_exists("volumes", &scratch_name).await?)
    })
    .await?;

    let template_name = format!("{}-template", plane.namespace());
    ceph.create_template(&template_name, "1M").await?;
    let clone_name = format!("{}-cloned", plane.namespace());
    volumes
        .create(
            &PostParams::default(),
            &Volume::new(
                "cloned",
                VolumeSpec {
                    size: "1 Mi".into(),
                    template: Some(template_name.clone()),
                },
            ),
        )
        .await?;
    wait_for("cloned RBD volume", || {
        ceph.image_exists("volumes", &clone_name)
    })
    .await?;
    assert_eq!(
        ceph.clone_parent("volumes", &clone_name).await?,
        Some(format!("templates/{template_name}@default")),
    );
    wait_for("cloned volume finalizer", || async {
        Ok(has_ceph_finalizer(
            &volumes.get("cloned").await?.metadata.finalizers,
        ))
    })
    .await?;
    volumes.delete("cloned", &Default::default()).await?;
    wait_for("removed cloned volume and Kubernetes object", || async {
        Ok(volumes.get_opt("cloned").await?.is_none()
            && !ceph.image_exists("volumes", &clone_name).await?)
    })
    .await?;

    // Creation from a URL is not implemented; pre-seed RBD to cover the
    // controller's existing-image and deletion paths instead.
    let image_name = format!("{}-preseeded", plane.namespace());
    ceph.create_image("templates", &image_name, "1M").await?;
    let images: Api<Image> = Api::namespaced(client.clone(), plane.namespace());
    let _image_controller = AbortOnDrop(tokio::spawn(images::create(client.clone())));
    images
        .create(
            &PostParams::default(),
            &Image::new(
                "preseeded",
                ImageSpec {
                    source: "unused".into(),
                },
            ),
        )
        .await?;
    wait_for("existing image finalizer", || async {
        Ok(has_ceph_finalizer(
            &images.get("preseeded").await?.metadata.finalizers,
        ))
    })
    .await?;
    assert!(ceph.image_exists("templates", &image_name).await?);
    images.delete("preseeded", &Default::default()).await?;
    wait_for("removed RBD image and Kubernetes object", || async {
        Ok(images.get_opt("preseeded").await?.is_none()
            && !ceph.image_exists("templates", &image_name).await?)
    })
    .await?;

    // Restart after the generated key is already present. A second controller
    // must still reconcile RBD volumes, and must not rewrite that Secret.
    volume_controller.0.abort();
    let _ = (&mut volume_controller.0).await;
    let _restarted_controller = AbortOnDrop(tokio::spawn(volumes::create(client)));
    let existing_key_name = format!("{}-existing-key", plane.namespace());
    volumes
        .create(
            &PostParams::default(),
            &Volume::new(
                "existing-key",
                VolumeSpec {
                    size: "1 Mi".into(),
                    template: None,
                },
            ),
        )
        .await?;
    wait_for(
        "RBD volume after controller restart with existing keyring",
        || ceph.image_exists("volumes", &existing_key_name),
    )
    .await?;
    wait_for("existing-key volume finalizer", || async {
        Ok(has_ceph_finalizer(
            &volumes.get("existing-key").await?.metadata.finalizers,
        ))
    })
    .await?;
    volumes.delete("existing-key", &Default::default()).await?;
    wait_for(
        "removed existing-key volume and Kubernetes object",
        || async {
            Ok(volumes.get_opt("existing-key").await?.is_none()
                && !ceph.image_exists("volumes", &existing_key_name).await?)
        },
    )
    .await?;
    let retained_secret = secrets.get(KEYRING_SECRET).await?;
    assert_eq!(retained_secret.data, secret_data);
    assert_eq!(retained_secret.metadata.resource_version, secret_version);
    Ok(())
}
