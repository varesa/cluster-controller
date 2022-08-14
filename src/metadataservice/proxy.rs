use std::fs::File;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::process::Command;
use std::env;

use nix::sched::{setns, CloneFlags};
use warp::Filter;

use crate::metadataservice::bidirectional_channel::ChannelEndpoint;
use crate::metadataservice::protocol::{ChannelProtocol, MetadataRequest};
use crate::Error;
use crate::utils::get_version_string;

fn ip_command(args: Vec<&str>) -> Result<(), Error> {
    let output = Command::new("/usr/sbin/ip")
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

fn create_interface(ns_name: &str) -> Result<(), Error> {
    let if_host = format!("todo-host");
    let if_ns = format!("todo-ns");

    ip_command(vec![
        "link", "add", &if_host, "type", "veth", "peer", &if_ns,
    ])?;
    ip_command(vec!["link", "set", &if_ns, "netns", ns_name])?;
    ip_command_netns(
        ns_name,
        vec!["addr", "add", "169.254.169.254/30", "dev", &if_ns],
    )?;
    ip_command_netns(ns_name, vec!["link", "set", &if_ns, "up"])?;
    ip_command(vec!["link", "set", &if_host, "up"])?;
    Ok(())
}

pub struct MetadataProxy {
    channel_endpoint: ChannelEndpoint<ChannelProtocol>,
}

impl MetadataProxy {
    pub fn run(
        channel_endpoint: ChannelEndpoint<ChannelProtocol>,
        namespace: &str,
    ) -> Result<(), Error> {
        if env::var_os("RUST_LOG").is_none() {
            // Set `RUST_LOG=todos=debug` to see debug logs,
            // this only shows access logs.
            env::set_var("RUST_LOG", "todos=info");
        }
        pretty_env_logger::init();

        println!("proxy: Starting metadata proxy");
        let ns = create_ns(namespace)?;
        create_interface(namespace)?;
        setns(ns.as_raw_fd(), CloneFlags::CLONE_NEWNET).map_err(Error::NetnsChangeFailed)?;

        let debug = Command::new("/usr/sbin/ip")
            .arg("addr")
            .output()
            .expect("Failed to list IP")
            .stdout;
        println!("{}", String::from_utf8(debug).unwrap());

        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut mp = MetadataProxy { channel_endpoint };
        rt.block_on(mp.main())?;
        Err(Error::UnexpectedExit(String::from("metadata proxy HTTP API (async main) died")))
    }

    pub async fn main(&mut self) -> Result<(), Error> {
        let root = warp::path::end().map(|| {
            format!("Metadata proxy from {}", get_version_string())
        });

        warp::serve(root.with(warp::log("api"))).run(([0,0,0,0], 80)).await;
        Err(Error::UnexpectedExit(String::from("metadata proxy HTTP API (warp) died")))
    }
}
