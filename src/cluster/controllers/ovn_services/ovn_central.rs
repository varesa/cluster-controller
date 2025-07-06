use crate::NAMESPACE;
use crate::cluster::controllers::ovn_services;
use crate::cluster::get_running_image;
use crate::errors::Error;
use crate::labels_and_annotations::OVN_CENTRAL_MANAGED_LABEL;
use k8s_openapi::api::apps::v1::DaemonSet;
use k8s_openapi::api::core::v1::{
    Container, EnvVar, PodSpec, PodTemplateSpec, ResourceRequirements, SecurityContext, Volume,
    VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use kube::{
    Client,
    api::{Api, Patch, PatchParams},
};
use std::collections::BTreeMap;
use tokio::time::{Duration, sleep};
use tracing::{info, instrument};

const OVN_CENTRAL_NAME: &str = "ovn-central";

fn container_base(image: String) -> Container {
    Container {
        image: Some(image),
        command: Some(vec!["bash".into()]),
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
        resources: Some(ResourceRequirements {
            requests: Some(BTreeMap::from([(
                "ephemeral-storage".to_string(),
                Quantity("128Mi".to_string()),
            )])),
            ..Default::default()
        }),
        security_context: Some(SecurityContext {
            privileged: Some(true),
            ..SecurityContext::default()
        }),
        ..Container::default()
    }
}

fn ovsdb_container(
    name: String,
    image: String,
    volume_mounts: Vec<VolumeMount>,
    db_name: String,
    file_prefix: String,
    client_port: u16,
    cluster_port: u16,
) -> Container {
    Container {
        name,
        args: Some(vec![
            "-c".into(),
            format!("set -euo pipefail
                            local_ip=\"$(ip -j address show lo | jq -r '.[0].addr_info[] | select(.scope == \"global\").local')\"
                            if [[ \"$local_ip\" != 10.* ]]; then
                                exit 1;
                            fi

                            test -f /var/lib/ovn/{file_prefix}.db || \
                                ovsdb-tool join-cluster /var/lib/ovn/{file_prefix}.db {db_name} \
                                    tcp:$local_ip:{cluster_port} tcp:10.4.0.31:{cluster_port} #temporary remote during migration

                            ovsdb-server \
                                -vconsole:info \
                                --pidfile=/var/run/ovn/{file_prefix}.pid \
                                --unixctl=/var/run/ovn/{file_prefix}.ctl \
                                --monitor \
                                --remote=punix:/var/run/ovn/{file_prefix}.sock \
                                --remote=ptcp:{client_port}:$local_ip /var/lib/ovn/{file_prefix}.db"),
        ]),
        volume_mounts: Some(volume_mounts),
        ..container_base(image)
    }
}

fn ovn_northd_container(
    image: String,
    volume_mounts: Vec<VolumeMount>,
    _ip_addresses: Vec<String>,
) -> Container {
    // TODO: Generate these lists automatically
    let nbdb_addresses =
        "tcp:10.4.3.1:6641,tcp:10.4.3.2:6641,tcp:10.4.3.4:6641,tcp:10.4.3.5:6641,tcp:10.4.3.6:6641";
    let sbdb_addresses =
        "tcp:10.4.3.1:6642,tcp:10.4.3.2:6642,tcp:10.4.3.4:6642,tcp:10.4.3.5:6642,tcp:10.4.3.6:6642";

    Container {
        name: "northd".into(),
        args: Some(vec![
            "-c".into(),
            format!(
                "set -euo pipefail
                         ovn-northd \
                                -vconsole:info \
                                --pidfile=/var/run/ovn/northd.pid \
                                --ovnnb-db={nbdb_addresses} \
                                --ovnsb-db={sbdb_addresses}"
            ),
        ]),
        volume_mounts: Some(volume_mounts),
        ..container_base(image)
    }
}
fn make_daemonset(image: String) -> Result<DaemonSet, Error> {
    let mut labels: BTreeMap<String, String> = BTreeMap::new();
    labels.insert("app".to_string(), OVN_CENTRAL_NAME.to_string());

    let mut node_selector = BTreeMap::new();
    node_selector.insert(OVN_CENTRAL_MANAGED_LABEL.to_string(), "managed".to_string());

    let host_volume_paths = [
        "/var/run/openvswitch",
        "/var/run/ovn",
        "/var/lib/ovn",
        "/etc/openvswitch",
    ];

    let volumes = host_volume_paths
        .iter()
        .map(|path| Volume {
            name: path[1..].replace("/", "-"),
            host_path: Some(k8s_openapi::api::core::v1::HostPathVolumeSource {
                path: path.to_string(),
                type_: Some("DirectoryOrCreate".to_string()),
            }),
            ..Volume::default()
        })
        .collect();

    let volume_mounts: Vec<VolumeMount> = host_volume_paths
        .iter()
        .map(|path| VolumeMount {
            name: path[1..].replace("/", "-"),
            mount_path: path.to_string(),
            ..VolumeMount::default()
        })
        .collect();

    let ds = DaemonSet {
        metadata: ObjectMeta {
            name: Some(OVN_CENTRAL_NAME.to_string()),
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
                    containers: vec![
                        ovsdb_container(
                            "nbdb".into(),
                            image.clone(),
                            volume_mounts.clone(),
                            "OVN_Northbound".into(),
                            "ovnnb_db".into(),
                            6641,
                            6643,
                        ),
                        ovsdb_container(
                            "sbdb".into(),
                            image.clone(),
                            volume_mounts.clone(),
                            "OVN_Southbound".into(),
                            "ovnsb_db".into(),
                            6642,
                            6644,
                        ),
                        ovn_northd_container(image, volume_mounts, vec![]),
                    ],
                    node_selector: Some(node_selector),
                    volumes: Some(volumes),
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
    };

    Ok(ds)
}

#[instrument(skip(client))]
pub async fn create(client: Client) -> Result<(), Error> {
    info!("Starting OVN central");

    let daemonsets: Api<DaemonSet> = Api::namespaced(client.clone(), NAMESPACE);

    let ovn_ds = make_daemonset(ovn_services::IMAGE.to_string())?;

    info!("daemonset definition formed");

    daemonsets
        .patch(
            OVN_CENTRAL_NAME,
            &PatchParams::apply("ovn-services"),
            &Patch::Apply(&ovn_ds),
        )
        .await?;

    info!("OVN central started, entering idle loop");
    loop {
        sleep(Duration::from_secs(60)).await;
    }
}
