use futures::StreamExt;
use kube::runtime::controller::{Context, Controller, ReconcilerAction};
use kube::{
    api::{Api, ListParams, PostParams},
    Client, ResourceExt,
};
use serde_json::json;
use tokio::time::Duration;

use crate::cluster::ovn::lowlevel::Ovn;
use crate::crd::ovn::{Network, NetworkStatus};
use crate::errors::Error;
use crate::utils::name_namespaced;
use crate::{
    api_replace_resource, client_ensure_finalizer, client_remove_finalizer, create_controller,
    create_set_status, resource_has_finalizer, GROUP_NAME,
};

/// State available for the reconcile and error_policy functions
/// called by the Controller
struct State {
    client: Client,
}

create_set_status!(Network, NetworkStatus);

fn ensure_exists(name: &str) {
    let mut ovn = Ovn::new("10.4.3.1", 6641);
    if ovn.get_ls(name).is_some() {
        println!("ovn: Sw {name} exists, OK");
    } else {
        println!("ovn: Sw {name} doesn't exist, creating");
        ovn.add_ls(name);
        println!("ovn: Sw {name} created, OK");
    }
}

fn delete(name: &str) -> Result<(), Error> {
    let mut ovn = Ovn::new("10.4.3.1", 6641);
    ovn.del_ls_by_name(name)?;
    Ok(())
}

/// Handle updates to networks in the cluster
async fn reconcile(network: Network, ctx: Context<State>) -> Result<ReconcilerAction, Error> {
    let client = ctx.get_ref().client.clone();
    let name = name_namespaced(&network);

    if network.metadata.deletion_timestamp.is_some() {
        println!("ovn: Network {} waiting for deletion", name);
        delete(&name)?;
        client_remove_finalizer!(client, Network, &network, "ovn");
    } else {
        println!("ovn: updated: {name}");
        client_ensure_finalizer!(client, Network, &network, "ovn");
        ensure_exists(&name);

        let status = NetworkStatus { is_created: true };
        set_status(&network, status, client.clone()).await?;
    }

    Ok(ReconcilerAction {
        requeue_after: Some(Duration::from_secs(600)),
    })
}

fn error_policy(_error: &Error, _ctx: Context<State>) -> ReconcilerAction {
    ReconcilerAction {
        requeue_after: Some(Duration::from_secs(15)),
    }
}

pub async fn create(client: Client) -> Result<(), Error> {
    let context = Context::new(State {
        client: client.clone(),
    });
    println!("ovn: Starting controller");

    let vms: Api<Network> = Api::all(client.clone());
    create_controller!(vms, reconcile, error_policy, context);
    Ok(())
}
