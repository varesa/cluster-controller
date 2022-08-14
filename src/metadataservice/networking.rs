use core::result::Result;
use core::result::Result::{Err, Ok};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::path::Path;
use std::process::Command;

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

pub fn create_ns(ns_name: &str) -> Result<File, Error> {
    let ns_path_str = format!("/var/run/netns/{}", ns_name);
    let ns_path = Path::new(&ns_path_str);

    println!("proxy: Trying to open {}", &ns_path_str);

    ip_command(vec!["netns", "add", ns_name]).map_err(|e| match e {
        Error::CommandError(_cmd, msg) => Error::NetnsCreateFailed(msg),
        e => e,
    })?;
    File::open(ns_path).map_err(Error::NetnsOpenFailed)
}

fn generate_interface_prefix(namespace: &str, router: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(namespace);
    hasher.update(router);
    let hex = format!("{:x}", hasher.finalize());
    hex[0..9].to_string()
}

pub fn create_interface(ns_name: &str, router_name: &str) -> Result<(), Error> {
    let prefix = generate_interface_prefix(ns_name, router_name);
    let if_host = format!("{prefix}-host");
    let if_ns = format!("{prefix}-ns");

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
