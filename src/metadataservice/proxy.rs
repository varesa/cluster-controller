use std::env;
use std::net::{Ipv4Addr, SocketAddr};
use std::os::unix::io::AsRawFd;

use nix::sched::{setns, CloneFlags};
use tokio::sync::mpsc::{channel, Sender};
use warp::{Filter, Rejection};

use crate::metadataservice::networking;
use crate::metadataservice::protocol::MetadataRequest;
use crate::utils::get_version_string;
use crate::Error;

pub struct MetadataProxy {
    channel_endpoint: Sender<MetadataRequest>,
}

impl MetadataProxy {
    pub fn run(channel_endpoint: Sender<MetadataRequest>, router_name: &str) -> Result<(), Error> {
        if env::var_os("RUST_LOG").is_none() {
            // Set `RUST_LOG=todos=debug` to see debug logs,
            // this only shows access logs.
            env::set_var("RUST_LOG", "todos=info");
        }
        pretty_env_logger::init();

        let netns_name = format!("{router_name}-metadatasvc");

        println!("proxy: Starting metadata proxy");
        let ns = networking::create_ns(&netns_name)?;
        networking::create_interface(&netns_name, router_name)?;
        setns(ns.as_raw_fd(), CloneFlags::CLONE_NEWNET).map_err(Error::NetnsChangeFailed)?;

        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut mp = MetadataProxy { channel_endpoint };
        rt.block_on(mp.main())?;
        Err(Error::UnexpectedExit(String::from(
            "metadata proxy HTTP API (async main) died",
        )))
    }

    fn fetch_metadata(
        &mut self,
    ) -> impl Filter<Extract = ((Ipv4Addr, String),), Error = Rejection> + Clone {
        warp::addr::remote()
            .and_then(|addr: Option<SocketAddr>| async move {
                match addr {
                    Some(SocketAddr::V4(addr4)) => Ok(*addr4.ip()),
                    _ => Err(warp::reject::not_found()),
                }
            })
            .and_then({
                let request_channel = self.channel_endpoint.clone();
                move |addr: Ipv4Addr| {
                    let request_channel = request_channel.clone();
                    async move {
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
                            Ok(metadata) => Ok((addr, metadata)),
                            Err(e) => {
                                println!("proxy: received error {:?}", e);
                                Err(warp::reject::not_found())
                            }
                        }
                    }
                }
            })
    }

    pub async fn main(&mut self) -> Result<(), Error> {
        let root =
            warp::path::end()
                .and(self.fetch_metadata())
                .map(|params: (Ipv4Addr, String)| {
                    let (addr, metadata) = params;
                    format!(
                        "Metadata proxy from {}\nClient IP: {}\nMetadata: {}\n",
                        get_version_string(),
                        addr,
                        metadata
                    )
                });

        let userdata = warp::path!("openstack" / "latest" / "user_data")
            .and(self.fetch_metadata())
            .map(|params: (Ipv4Addr, String)| params.1);

        let app = root.or(userdata).with(warp::log("api"));
        warp::serve(app).run(([0, 0, 0, 0], 80)).await;
        Err(Error::UnexpectedExit(String::from(
            "metadata proxy HTTP API (warp) died",
        )))
    }
}
