use crate::cluster::get_running_image;
use crate::errors::Error;
use crate::labels_and_annotations::OVN_CONTROLLER_MANAGEMENT_LABEL;
use crate::NAMESPACE;
use k8s_openapi::api::apps::v1::DaemonSet;
use k8s_openapi::api::core::v1::{
    Container, EmptyDirVolumeSource, EnvVar, PodSpec, PodTemplateSpec, SecurityContext, Volume,
    VolumeMount,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use kube::{
    api::{Api, Patch, PatchParams},
    Client,
};
use std::collections::BTreeMap;
use tokio::time::{sleep, Duration};
use tracing::{info, instrument};

const OVN_CONTROLLER_NAME: &str = "ovn-controller";

fn make_ovn_daemonset(image: String) -> Result<DaemonSet, Error> {
    let mut labels: BTreeMap<String, String> = BTreeMap::new();
    labels.insert("app".to_string(), OVN_CONTROLLER_NAME.to_string());

    let mut node_selector = BTreeMap::new();
    node_selector.insert(
        OVN_CONTROLLER_MANAGEMENT_LABEL.to_string(),
        "managed".to_string(),
    );

    Ok(DaemonSet {
        metadata: ObjectMeta {
            name: Some(OVN_CONTROLLER_NAME.to_string()),
            namespace: Some(NAMESPACE.to_string()),
            labels: Some(labels.clone()),
            ..Default::default()
        },
        spec: Some(k8s_openapi::api::apps::v1::DaemonSetSpec {
            selector: LabelSelector {
                match_labels: Some(labels.clone()),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: OVN_CONTROLLER_NAME.to_string(),
                        image: Some(image),
                        command: Some(vec!["ovn-controller".into()]),
                        args: Some(vec![
                            "unix:/var/run/openvswitch/db.sock".into(),
                            "-vfile:info".into(),
                            "--no-chdir".into(),
                            "--log-file=/dev/stdout".into(),
                            "--pidfile=/var/run/ovn/ovn-controller.pid".into(),
                            "--monitor".into(),
                        ]),
                        env: Some(vec![
                            EnvVar {
                                name: "OVN_RUNDIR".to_string(),
                                value: Some("/var/run/ovn".to_string()),
                                ..EnvVar::default()
                            },
                            EnvVar {
                                name: "OVS_RUNDIR".to_string(),
                                value: Some("/var/run/openvswitch".to_string()),
                                ..EnvVar::default()
                            },
                        ]),
                        volume_mounts: Some(vec![
                            VolumeMount {
                                name: "ovs-run".to_string(),
                                mount_path: "/var/run/openvswitch".to_string(),
                                ..VolumeMount::default()
                            },
                            VolumeMount {
                                name: "ovn-run".to_string(),
                                mount_path: "/var/run/ovn".to_string(),
                                ..VolumeMount::default()
                            },
                        ]),
                        security_context: Some(SecurityContext {
                            privileged: Some(true),
                            ..SecurityContext::default()
                        }),
                        ..Container::default()
                    }],
                    node_selector: Some(node_selector),
                    volumes: Some(vec![
                        Volume {
                            name: "ovs-run".to_string(),
                            host_path: Some(k8s_openapi::api::core::v1::HostPathVolumeSource {
                                path: "/var/run/openvswitch".to_string(),
                                type_: Some("Directory".to_string()),
                            }),
                            ..Volume::default()
                        },
                        Volume {
                            name: "ovn-run".to_string(),
                            empty_dir: Some(EmptyDirVolumeSource {
                                ..EmptyDirVolumeSource::default()
                            }),
                            ..Volume::default()
                        },
                    ]),
                    host_network: Some(true),
                    dns_policy: Some("ClusterFirstWithHostNet".to_string()),
                    ..PodSpec::default()
                }),
            },
            update_strategy: Some(k8s_openapi::api::apps::v1::DaemonSetUpdateStrategy {
                type_: Some("RollingUpdate".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }),
        status: None,
    })
}

#[instrument(skip(client))]
pub async fn create(client: Client) -> Result<(), Error> {
    info!("Starting OVN controller");

    let daemonsets: Api<DaemonSet> = Api::namespaced(client.clone(), NAMESPACE);

    let ovn_ds = make_ovn_daemonset(get_running_image(client.clone()).await?)?;

    daemonsets
        .patch(
            OVN_CONTROLLER_NAME,
            &PatchParams::apply("ovn-controller"),
            &Patch::Apply(&ovn_ds),
        )
        .await?;

    info!("OVN controller started, entering idle loop");
    loop {
        sleep(Duration::from_secs(60)).await;
    }
}
