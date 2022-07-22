use crate::crd::cluster::Cluster;
use crate::crd::libvirt::v1beta2::VirtualMachine;
use crate::errors::ClusterNotFound;
use crate::host::libvirt::controller::State;
use crate::Error;
use kube::{Api, ResourceExt};
use lazy_static::lazy_static;
use regex::Regex;
use std::sync::Arc;

/// Construct the expected
pub fn get_domain_name(vm: &VirtualMachine) -> Option<String> {
    let domain_name: Option<&str> = vm.status.as_ref().map(|status| status.domain_name.as_ref());
    match domain_name {
        Some(name) => Some(String::from(name)),
        _ => {
            let namespace = ResourceExt::namespace(vm)
                .or_else(|| Some(String::from("<no namespace>")))
                .unwrap();
            println!(
                "Ignored VM {}/{} with no domain name defined",
                namespace,
                vm.metadata.name.as_ref().expect("get VM name")
            );
            None
        }
    }
}

pub async fn get_cluster(ctx: &Arc<State>) -> Result<Cluster, ClusterNotFound> {
    let name: &str = "default";
    let client = ctx.kube.clone();
    let clusters: Api<Cluster> = Api::all(client.clone());
    let default = clusters.get(name).await;

    match default {
        Ok(cluster) => Ok(cluster),
        Err(error) => Err(ClusterNotFound {
            name: name.into(),
            inner_error: error,
        }),
    }
}

pub fn parse_memory(input: &str) -> Result<(usize, String), Error> {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"(\d+)\s*([a-zA-Z]+)").unwrap();
    }
    let captures = RE.captures(input).unwrap();
    Ok((
        captures.get(1).unwrap().as_str().parse().unwrap(),
        captures.get(2).unwrap().as_str().to_string(),
    ))
}
