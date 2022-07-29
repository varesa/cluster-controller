use std::fs::File;
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use futures::SinkExt;
use nix::sched::{setns, CloneFlags};

use crate::metadataservice::bidirectional_channel::ChannelEndpoint;
use crate::metadataservice::protocol::{ChannelProtocol, MetadataRequest};
use crate::Error;

fn get_ns(ns_name: &str) -> Result<File, Error> {
    let ns_path_str = format!("/var/run/netns/{}", ns_name);
    let ns_path = Path::new(&ns_path_str);

    println!("proxy: Trying to open {}", &ns_path_str);
    let file = match File::open(ns_path) {
        Ok(file) => file,
        Err(_) => {
            println!("proxy: Open failed, trying to create");
            let output = Command::new("/usr/sbin/ip")
                .arg("netns")
                .arg("add")
                .arg(ns_name)
                .output()
                .expect("Failed to create netns");
            if !output.status.success() {
                return Err(Error::NetnsCreateFailed(
                    String::from_utf8(output.stderr).expect("stderr not valid UTF-8"),
                ));
            }
            File::open(ns_path).expect("Failed to open netns")
        }
    };

    Ok(file)
}

pub struct MetadataProxy {
    channel_endpoint: ChannelEndpoint<ChannelProtocol>,
}

impl MetadataProxy {
    pub fn run(
        channel_endpoint: ChannelEndpoint<ChannelProtocol>,
        namespace: &str,
    ) -> Result<(), Error> {
        println!("proxy: Starting metadata proxy");
        let ns = get_ns(namespace)?;
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
        Ok(())
    }

    pub async fn main(&mut self) -> Result<(), Error> {
        self.channel_endpoint
            .tx
            .send(ChannelProtocol::MetadataRequest(MetadataRequest {
                ip: String::from("127.0.0.1"),
            }))
            .await?;

        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}
