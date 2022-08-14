use std::env;
use std::net::SocketAddr;
use std::os::unix::io::AsRawFd;

use nix::sched::{setns, CloneFlags};
use tokio::sync::mpsc::{channel, Sender};
use warp::Filter;

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

    pub async fn main(&mut self) -> Result<(), Error> {
        let root = warp::addr::remote()
            .and(warp::path::end())
            .and_then(|addr: Option<SocketAddr>| async move {
                match addr {
                    Some(SocketAddr::V4(addr4)) => Ok(addr4.ip().to_string()),
                    _ => Err(warp::reject::not_found()),
                }
            })
            .then({
                let request_channel = self.channel_endpoint.clone();
                move |addr: String| {
                    let request_channel = request_channel.clone();
                    async move {
                        let (return_sender, mut return_receiver) = channel(1);
                        request_channel
                            .send(MetadataRequest {
                                ip: addr.clone(),
                                return_channel: return_sender,
                            })
                            .await
                            .expect("Failed to send metadata request");
                        let response = return_receiver
                            .recv()
                            .await
                            .expect("Failed to get metadata response");
                        let resp = format!(
                            "Metadata proxy from {}\nClient IP: {}\nMetadata: {}\n",
                            get_version_string(),
                            addr,
                            response.metadata
                        );
                        Ok(resp)
                    }
                }
            });

        warp::serve(root.with(warp::log("api")))
            .run(([0, 0, 0, 0], 80))
            .await;
        Err(Error::UnexpectedExit(String::from(
            "metadata proxy HTTP API (warp) died",
        )))
    }
}
