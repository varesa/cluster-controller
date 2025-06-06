use std::net::{Ipv4Addr, SocketAddr};
use std::os::unix::io::AsRawFd;

use crate::Error;
use crate::metadataservice::networking;
use crate::metadataservice::protocol::{MetadataPayload, MetadataRequest};
use crate::utils::strings::get_version_string;
use nix::sched::{CloneFlags, setns};
use serde_json::json;
use tokio::sync::mpsc::{Sender, channel};
use warp::filters::BoxedFilter;
use warp::{Filter, Rejection, Reply};

pub struct MetadataProxy {
    channel_endpoint: Sender<MetadataRequest>,
}

impl MetadataProxy {
    /// Main entrypoint
    ///
    /// - Create a new netns and move there
    /// - Start the proxy
    pub fn run(channel_endpoint: Sender<MetadataRequest>, router_name: &str) -> Result<(), Error> {
        let netns_name = format!("{router_name}-metadatasvc");

        println!("proxy: Starting metadata proxy");
        let ns = networking::create_ns(&netns_name)?;
        networking::create_interface(&netns_name, router_name)?;
        setns(ns.as_raw_fd(), CloneFlags::CLONE_NEWNET).map_err(Error::NetnsChangeFailed)?;

        let rt = tokio::runtime::Runtime::new().unwrap();
        let mp = MetadataProxy { channel_endpoint };
        rt.block_on(mp.main())?;
        Err(Error::UnexpectedExit(String::from(
            "metadata proxy HTTP API (async main) died",
        )))
    }

    /// A warp filter which resolves the client IP address into a MetadataPayload
    fn addr_to_metadata(
        &self,
    ) -> impl Filter<Extract = (MetadataPayload,), Error = Rejection> + Clone + use<> {
        warp::addr::remote()
            // [0] and_then expects a callable which returns a future
            .and_then(extract_ipv4_address)
            // [1] so here we create a block...
            .and_then({
                // Clone the borrowed channel to a owned value so it can be moved to the closure
                let request_channel = self.channel_endpoint.clone();

                // [2] which returns a sync closure
                move |addr: Ipv4Addr| {
                    // Make a per-request copy of the channel saved in the closure
                    let request_channel = request_channel.clone();
                    // [3] which returns a future
                    fetch_metadata(addr, request_channel)
                }
            })
    }

    pub async fn main(self) -> Result<(), Error> {
        // Builtin default page

        let root =
            warp::path::end()
                .and(self.addr_to_metadata())
                .map(|metadata: MetadataPayload| {
                    format!(
                        "Metadata proxy from {}\nClient IP: {}\nInstance ID: {}\nHostname: {}\nMetadata: {}\n",
                        get_version_string(),
                        metadata.ip,
                        metadata.instance_id,
                        metadata.hostname,
                        metadata.user_data,
                    )
                });

        let app = root
            .or(warp::path("openstack").and(self.openstack_api()))
            .with(warp::log("api"));
        warp::serve(app).run(([0, 0, 0, 0], 80)).await;
        Err(Error::UnexpectedExit(String::from(
            "metadata proxy HTTP API (warp) died",
        )))
    }

    fn openstack_api(&self) -> BoxedFilter<(impl Reply + use<>,)> {
        let openstack_root = warp::path::end().map(|| String::from("latest"));

        let openstack_latest =
            warp::path!("latest").map(|| String::from("meta_data.json\nuser_data"));

        let openstack_latest_metadata = warp::path!("latest" / "meta_data.json")
            .and(self.addr_to_metadata())
            .map(|metadata: MetadataPayload| {
                json!({
                    "uuid": metadata.instance_id,
                    "hostname": metadata.hostname
                })
                .to_string()
            });

        let openstack_latest_userdata = warp::path!("latest" / "user_data")
            .and(self.addr_to_metadata())
            .map(|metadata: MetadataPayload| metadata.user_data);

        openstack_root
            .or(openstack_latest)
            .or(openstack_latest_metadata)
            .or(openstack_latest_userdata)
            .boxed()
    }
}

/// Convert a SocketAddr into an IPv4Addr
async fn extract_ipv4_address(addr: Option<SocketAddr>) -> Result<Ipv4Addr, Rejection> {
    match addr {
        Some(SocketAddr::V4(addr4)) => Ok(*addr4.ip()),
        _ => Err(warp::reject::not_found()),
    }
}

/// Send a MetadataRequest over a channel to the backend and wait for a MetadataResponse back.
/// Return the wrapped MetadataPayload
async fn fetch_metadata(
    addr: Ipv4Addr,
    request_channel: Sender<MetadataRequest>,
) -> Result<MetadataPayload, Rejection> {
    let (return_sender, mut return_receiver) = channel(1);
    request_channel
        .send(MetadataRequest {
            ip: addr,
            return_channel: return_sender,
        })
        .await
        .expect("Failed to send metadata request");
    let response = return_receiver
        .recv()
        .await
        .expect("Failed to get metadata response");

    match *response.metadata {
        // Repackage Result<_,Error> into Result<_,Rejection>
        Ok(metadata) => Ok(metadata),
        Err(e) => {
            println!("proxy: received error {e:?}");
            Err(warp::reject::not_found())
        }
    }
}
