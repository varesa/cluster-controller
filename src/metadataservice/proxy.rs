use std::env;
use std::fs::File;
use std::net::SocketAddr;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use nix::sched::{setns, CloneFlags};
use warp::Filter;

use crate::metadataservice::bidirectional_channel::ChannelEndpoint;
use crate::metadataservice::protocol::{ChannelProtocol, MetadataRequest};
use crate::utils::get_version_string;
use crate::Error;

fn command(executable: &str, args: Vec<&str>) -> Result<(), Error> {
    let output = Command::new(executable)
        .args(&args)
        .output()
        .expect("failed to run command");

    if !output.status.success() {
        return Err(Error::CommandError(
            args.into_iter().map(|s| s.to_owned()).collect(),
            String::from_utf8(output.stderr).expect("stderr not valid UTF-8"),
        ));
    }
    Ok(())
}

fn ip_command(args: Vec<&str>) -> Result<(), Error> {
    command("/usr/sbin/ip", args)
}

fn ip_command_netns(netns: &str, args: Vec<&str>) -> Result<(), Error> {
    let mut command = vec!["netns", "exec", netns, "ip"];
    command.append(&mut args.clone());
    ip_command(command)
}

fn create_ns(ns_name: &str) -> Result<File, Error> {
    let ns_path_str = format!("/var/run/netns/{}", ns_name);
    let ns_path = Path::new(&ns_path_str);

    println!("proxy: Trying to open {}", &ns_path_str);

    ip_command(vec!["netns", "add", ns_name]).map_err(|e| match e {
        Error::CommandError(_cmd, msg) => Error::NetnsCreateFailed(msg),
        e => e,
    })?;
    File::open(ns_path).map_err(Error::NetnsOpenFailed)
}

fn create_interface(ns_name: &str, router_name: &str) -> Result<(), Error> {
    let if_host = format!("todo-host");
    let if_ns = format!("todo-ns");

    ip_command_netns(ns_name, vec!["link", "set", "lo", "up"])?;

    ip_command(vec![
        "link", "add", &if_host, "type", "veth", "peer", &if_ns,
    ])?;
    ip_command(vec!["link", "set", &if_ns, "netns", ns_name])?;
    ip_command_netns(
        ns_name,
        vec!["addr", "add", "169.254.169.254/30", "dev", &if_ns],
    )?;
    ip_command_netns(
        ns_name,
        vec!["link", "set", "dev", &if_ns, "address", "02:00:00:00:00:02"],
    )?;
    ip_command_netns(ns_name, vec!["link", "set", &if_ns, "up"])?;
    ip_command_netns(
        ns_name,
        vec!["route", "add", "default", "via", "169.254.169.253"],
    )?;
    ip_command(vec!["link", "set", &if_host, "up"])?;

    command("/usr/bin/ovs-vsctl", vec!["add-port", "br-int", &if_host]).or_else(|err| {
        if let Error::CommandError(_cmd, msg) = &err {
            if msg == &format!("ovs-vsctl: cannot create a port named todo-host because a port named {} already exists on bridge br-int\n", if_host) {
                return Ok(())
            }
        }
        Err(err)
    })?;
    command(
        "/usr/bin/ovs-vsctl",
        vec![
            "set",
            "Interface",
            &if_host,
            &format!("external_ids:iface-id=mds-{}", router_name),
        ],
    )?;
    Ok(())
}

pub struct MetadataProxy {
    channel_endpoint: Arc<ChannelEndpoint<ChannelProtocol>>,
}

impl MetadataProxy {
    pub fn run(
        channel_endpoint: ChannelEndpoint<ChannelProtocol>,
        router_name: &str,
    ) -> Result<(), Error> {
        if env::var_os("RUST_LOG").is_none() {
            // Set `RUST_LOG=todos=debug` to see debug logs,
            // this only shows access logs.
            env::set_var("RUST_LOG", "todos=info");
        }
        pretty_env_logger::init();

        let netns_name = format!("{router_name}-metadatasvc");

        println!("proxy: Starting metadata proxy");
        let ns = create_ns(&netns_name)?;
        create_interface(&netns_name, router_name)?;
        setns(ns.as_raw_fd(), CloneFlags::CLONE_NEWNET).map_err(Error::NetnsChangeFailed)?;

        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut mp = MetadataProxy {
            channel_endpoint: Arc::new(channel_endpoint),
        };
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
                    _ => return Err(warp::reject::not_found()),
                }
            })
            .then({
                let channel = self.channel_endpoint.clone();
                move |addr: String| {
                    let channel = channel.clone();
                    async move {
                        channel
                            .tx
                            .send(ChannelProtocol::MetadataRequest(MetadataRequest {
                                ip: addr.clone(),
                            }))
                            .await;
                        let resp = format!(
                            "Metadata proxy from {}\nClient IP: {}\n",
                            get_version_string(),
                            addr
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
