use futures::StreamExt;
use kube::runtime::controller::{Context, Controller, ReconcilerAction};
use kube::{
    api::{Api, ListParams},
    Client,
};
use tokio::time::Duration;

use crate::cluster::ovn::lowlevel::Ovn;
use crate::crd::ovn::Network;
use crate::create_controller;
use crate::errors::Error;
use crate::utils::name_namespaced;

/// State available for the reconcile and error_policy functions
/// called by the Controller
struct State {
    client: Client,
}

/// Handle updates to networks in the cluster
async fn reconcile(network: Network, ctx: Context<State>) -> Result<ReconcilerAction, Error> {
    let _client = ctx.get_ref().client.clone();
    let name = name_namespaced(&network);

    println!("ovn: updated: {name}");

    let mut ovn = Ovn::new("10.4.3.1", 6641);
    if ovn.list_ls().iter().any(|sw| sw.name == name) {
        println!("ovn: Sw {name} exists, OK");
    } else {
        println!("ovn: Sw {name} doesn't exist, creating");
        ovn.add_ls(&name);
        println!("ovn: Sw {name} created, OK");
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
