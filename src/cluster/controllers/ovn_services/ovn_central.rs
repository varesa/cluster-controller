use crate::NAMESPACE;
use crate::cluster::get_running_image;
use crate::errors::Error;
use crate::labels_and_annotations::OVN_CENTRAL_MANAGED_LABEL;
use k8s_openapi::api::apps::v1::DaemonSet;
use k8s_openapi::api::core::v1::{
    Container, EnvVar, PodSpec, PodTemplateSpec, SecurityContext, Volume, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use kube::{
    Client,
    api::{Api, Patch, PatchParams},
};
use std::collections::BTreeMap;
use tokio::time::{Duration, sleep};
use tracing::{info, instrument};

const OVN_CENTRAL_NAME: &str = "ovn-central";

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

    let volume_mounts = host_volume_paths
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
                    containers: vec![Container {
                        name: OVN_CENTRAL_NAME.to_string(),
                        image: Some(image),
                        command: Some(vec!["bash".into()]),
                        args: Some(vec![
                            "-c".into(),
                            "local_ip=\"$(ip -j address show lo | jq -r '.[0].addr_info[] | select(.scope == \"global\").local')\"
                            if [[ \"$local_ip\" != 10.* ]]; then
                                exit 1;
                            fi

                            DB=\"/var/lib/ovn/ovnnb_db.db\"\
                            NAME=\"OVN_Northbound\"
                            ovsdb-tool db-cid $DB || \
                                ovdb-tool join-cluster $DB $NAME tcp:$local_ip:6641 tcp:10.4.3.1:6641

                            ovsdb-server \
                                -vconsole:info \
                                --pidfile=/var/run/ovn/ovnnb_db.pid \
                                --unixctl=/var/run/ovn/ovnnb_db.ctl \
                                --monitor \
                                --remote=punix:/var/run/ovn/ovnnb_db.sock \
                                --remote=db:$NAME,NB_Global,connections \
                                --remote=ptcp:6641:$local_ip $DB \
                            ".into(),
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
                        volume_mounts: Some(volume_mounts),
                        security_context: Some(SecurityContext {
                            privileged: Some(true),
                            ..SecurityContext::default()
                        }),
                        ..Container::default()
                    }],
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

    let ovn_ds = make_daemonset(get_running_image(client.clone()).await?)?;

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
