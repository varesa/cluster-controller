use std::sync::Arc;

use serde_json::{json, Value};

use crate::cluster::ovn::common::{OvnCommon, OvnNamed};
use crate::cluster::ovn::dhcpoptions::DhcpOptions;
use crate::cluster::ovn::lowlevel::{Ovn, TYPE_LOGICAL_SWITCH_PORT};
use crate::{try_deserialize, Error};

pub struct LogicalSwitchPort {
    ovn: Arc<Ovn>,
    uuid: String,
    name: String,
}

impl LogicalSwitchPort {
    pub fn set_address(&mut self, mac_address: &str) -> Result<(), Error> {
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
        let object = try_deserialize!(value.as_object());

        Ok(LogicalSwitchPort {
            ovn,
            uuid: try_deserialize!(object
                .get("_uuid")
                .and_then(|a| a.as_array())
                .and_then(|a| a.get(1))
                .and_then(|u| u.as_str()))
            .to_owned(),
            name: try_deserialize!(object.get("name").and_then(|u| u.as_str())).to_owned(),
        })
    }
}

impl OvnNamed for LogicalSwitchPort {
    fn name(&self) -> String {
        self.name.clone()
    }
}
