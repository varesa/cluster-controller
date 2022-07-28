use std::sync::Arc;

use serde_json::{json, Value};

use crate::cluster::ovn::common::{OvnCommon, OvnNamed};
use crate::cluster::ovn::deserialization::{
    deserialize_object, deserialize_string, deserialize_uuid,
};
use crate::cluster::ovn::dhcpoptions::DhcpOptions;
use crate::cluster::ovn::lowlevel::{Ovn, TYPE_LOGICAL_SWITCH_PORT};
use crate::Error;

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
        let object = deserialize_object(value)?;

        Ok(LogicalSwitchPort {
            ovn,
            uuid: deserialize_uuid(object)?,
            name: deserialize_string(object, "name")?,
        })
    }
}

impl OvnNamed for LogicalSwitchPort {
    fn name(&self) -> String {
        self.name.clone()
    }
}
