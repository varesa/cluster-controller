use crate::crd::virtualmachine::{NetworkAttachment, VirtualMachine};
use crate::errors::Error;
use std::process::Command;

/// Make sure that all the VLANs requested by the VM are configured on the EVPN bridge
/// and VXLAN intefraces.
///
/// Runs the equivalent of:
///
/// /usr/sbin/bridge vlan add dev br0 vid 2102 self
/// /usr/sbin/bridge vlan add dev vxlan0 vid 2102
/// /usr/sbin/bridge vlan add dev vxlan0 vid 2102 tunnel_info id 2102
///
pub fn ensure_vni_mapping(vm: &VirtualMachine) -> Result<(), Error> {
    let nics: Vec<NetworkAttachment> = vm
        .status
        .as_ref()
        .map(|status| status.networks.clone())
        .unwrap_or_default();

    let mut vlans = Vec::new();
    for nic in nics {
        if let Some(mut tagged_vlans) = nic.tagged_vlans {
            vlans.append(&mut tagged_vlans);
        }
        if let Some(untagged_vlan) = nic.untagged_vlan {
            vlans.push(untagged_vlan);
        }
    }

    for vlan in vlans {
        #[rustfmt::skip]
        let add_to_bridge = Command::new("/usr/sbin/bridge")
            .args(["vlan",  "add", "dev", "br0", "vid", &vlan.to_string(), "self"])
            .output()?;

        if !add_to_bridge.status.success() {
            return Err(Error::VniMapping(format!(
                "add_to_bridge: {}",
                String::from_utf8_lossy(&add_to_bridge.stderr)
            )));
        }

        let add_to_vxlan = Command::new("/usr/sbin/bridge")
            .args(["vlan", "add", "dev", "vxlan0", "vid", &vlan.to_string()])
            .output()?;

        if !add_to_vxlan.status.success() {
            return Err(Error::VniMapping(format!(
                "add_to_vxlan: {}",
                String::from_utf8_lossy(&add_to_vxlan.stderr)
            )));
        }

        #[rustfmt::skip]
        let set_tunnel_info = Command::new("/usr/sbin/bridge")
            .args(["vlan", "add", "dev", "vxlan0", "vid", &vlan.to_string(), "tunnel_info", "id", &vlan.to_string()])
            .output()?;

        if !set_tunnel_info.status.success()
            && String::from_utf8_lossy(&set_tunnel_info.stderr)
                != *"RTNETLINK answers: File exists\n"
        {
            return Err(Error::VniMapping(format!(
                "set_tunnel_info: {}",
                String::from_utf8_lossy(&set_tunnel_info.stderr)
            )));
        }
    }

    Ok(())
}
