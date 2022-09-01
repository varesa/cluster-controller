use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::cluster::ovn::common::{OvnCommon, OvnGetters, OvnNamed, OvnNamedGetters};
use crate::cluster::ovn::deserialization::{
    deserialize_object, deserialize_string, deserialize_uuid,
};
use crate::cluster::ovn::dhcpoptions::DhcpOptions;
use crate::cluster::ovn::logicalswitch::LogicalSwitch;
use crate::cluster::ovn::lowlevel::{Ovn, TYPE_LOGICAL_SWITCH, TYPE_LOGICAL_SWITCH_PORT};
use crate::Error;
use crate::Error::OvnConflict;

pub struct LogicalSwitchPortBuilder<'a> {
    pub ovn: Arc<Ovn>,
    pub ls: &'a LogicalSwitch,
}

impl LogicalSwitchPortBuilder<'_> {
    pub fn create(
        self,
        lsp_name: &str,
        extra_params: Option<&Map<String, Value>>,
    ) -> Result<LogicalSwitchPort, Error> {
        let mut params = if let Some(extra_params) = extra_params {
            extra_params.clone()
        } else {
            Map::new()
        };
        params.insert("name".to_string(), Value::String(lsp_name.to_string()));

        let add_lsp = json!({
            "op": "insert",
            "table": TYPE_LOGICAL_SWITCH_PORT,
            "row": params,
            "uuid-name": "new_lsp"
        });

        let add_lsp_to_ls = json!({
            "op": "mutate",
            "table": TYPE_LOGICAL_SWITCH,
            "where": [
                ["_uuid", "==", ["uuid", self.ls.uuid()]]
            ],
            "mutations": [
                ["ports", "insert", ["set", [["named-uuid", "new_lsp"]]]]
            ]
        });
        self.ovn.transact(&[add_lsp, add_lsp_to_ls]);
        LogicalSwitchPort::get_by_name(self.ovn, lsp_name)
    }

    pub fn create_if_missing(
        self,
        lsp_name: &str,
        extra_params: Option<&Map<String, Value>>,
    ) -> Result<LogicalSwitchPort, Error> {
        match LogicalSwitchPort::get_by_name(self.ovn.clone(), lsp_name) {
            Ok(ls) => Ok(ls),
            Err(Error::OvnNotFound(_, _)) => {
                println!(
                    "ovn: {} {} doesn't exist, creating",
                    LogicalSwitchPort::ovn_type(),
                    lsp_name
                );

                self.create(lsp_name, extra_params)
            }
            Err(e) => Err(e),
        }
    }

    pub fn get_by_mac(self, mac_address: &str) -> Result<LogicalSwitchPort, Error> {
        let mut all_lsps = LogicalSwitchPort::list(self.ovn.clone())?;
        let ls_port_ids = self.ls.port_ids();
        all_lsps.retain(|lsp| ls_port_ids.contains(&lsp.uuid()));

        all_lsps
            .into_iter()
            .find(|lsp| lsp.addresses.contains(mac_address))
            .ok_or_else(|| {
                Error::OvnNotFound(LogicalSwitchPort::ovn_type(), mac_address.to_string())
            })
    }
}

#[derive(Debug)]
pub struct LogicalSwitchPort {
    ovn: Arc<Ovn>,
    uuid: String,
    name: String,
    addresses: String,
    dynamic_addresses: String,
}

impl LogicalSwitchPort {
    pub fn set_address(&mut self, mac_address: &str) -> Result<(), Error> {
        // Ignore if already set
        if self.addresses.contains(mac_address) {
            return Ok(());
        }

        // Fail if already in use elsewhere
        let mut ls = self.get_ls()?;
        if ls.lsp().get_by_mac(mac_address).is_ok() {
            return Err(OvnConflict(mac_address.to_string()));
        }

        // Otherwise set
        let set_address = json!({
            "op": "update",
            "table": TYPE_LOGICAL_SWITCH_PORT,
            "where": [
                [ "_uuid", "==", [ "uuid", self.uuid() ] ]
            ],
            "row": { "addresses": format!("{mac_address} dynamic") }
        });
        self.ovn.transact(&[set_address]);
        Ok(())
    }

    pub fn set_dhcp_options(&mut self, cidr: &str) -> Result<(), Error> {
        let dhcp_options = DhcpOptions::get_by_cidr(self.ovn.clone(), cidr)?;
        let set_dhcp_options = json!({
            "op": "update",
            "table": "Logical_Switch_Port",
            "where": [["_uuid", "==", ["uuid", self.uuid()]]],
            "row": {"dhcpv4_options":["uuid", dhcp_options.uuid()]}
        });

        self.ovn.transact(&[set_dhcp_options]);
        Ok(())
    }

    pub fn dynamic_ip(&self) -> Option<String> {
        let split: Vec<&str> = self.dynamic_addresses.split(' ').collect();
        split.get(1).map(|s| String::from(*s))
    }

    pub fn get_by_ip(ovn: Arc<Ovn>, ip: String) -> Result<Vec<LogicalSwitchPort>, Error> {
        let ports = Self::list(ovn)?;
        let ports_with_ip: Vec<LogicalSwitchPort> = ports
            .into_iter()
            .filter(|port| port.dynamic_ip().unwrap_or_else(|| String::from("")) == ip)
            .collect();
        Ok(ports_with_ip)
    }

    pub fn get_ls(&self) -> Result<LogicalSwitch, Error> {
        LogicalSwitch::find_lsp_owner(self.ovn.clone(), self)
    }
}

impl OvnCommon for LogicalSwitchPort {
    fn uuid(&self) -> String {
        self.uuid.clone()
    }

    fn ovn(&self) -> Arc<Ovn> {
        self.ovn.clone()
    }

    fn ovn_type() -> String {
        TYPE_LOGICAL_SWITCH_PORT.to_owned()
    }

    fn deserialize(ovn: Arc<Ovn>, value: &Value) -> Result<Self, Error> {
        let object = deserialize_object(value)?;

        Ok(LogicalSwitchPort {
            ovn,
            uuid: deserialize_uuid(object)?,
            name: deserialize_string(object, "name")?,
            addresses: deserialize_string(object, "addresses")
                .or_else(|_| Ok::<String, Error>(String::new()))?,
            dynamic_addresses: deserialize_string(object, "dynamic_addresses")
                .or_else(|_| Ok::<String, Error>(String::new()))?,
        })
    }
}

impl OvnNamed for LogicalSwitchPort {
    fn name(&self) -> String {
        self.name.clone()
    }
}
