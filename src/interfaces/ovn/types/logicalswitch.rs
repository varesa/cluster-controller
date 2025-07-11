use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::interfaces::ovn::common::{
    OvnBasicType, OvnCommon, OvnGetters, OvnNamed, OvnNamedGetters,
};
use crate::interfaces::ovn::deserialization::{
    deserialize_map, deserialize_object, deserialize_string, deserialize_uuid, deserialize_uuid_set,
};
use crate::interfaces::ovn::lowlevel::{Ovn, TYPE_LOGICAL_SWITCH};
use crate::interfaces::ovn::types::logicalswitchport::{
    LogicalSwitchPort, LogicalSwitchPortBuilder,
};
use crate::Error;

pub struct LogicalSwitch {
    ovn: Arc<Ovn>,
    uuid: String,
    name: String,
    port_ids: Vec<String>,
    other_config: HashMap<String, String>,
}

impl LogicalSwitch {
    pub fn set_cidr(&mut self, cidr: &str) -> Result<(), Error> {
        let set_cidr = json!({
            "op": "update",
            "table": "Logical_Switch",
            "where": [["_uuid", "==", ["uuid", self.uuid()]]],
            "row": {"other_config": ["map", [["subnet", cidr]]]}
        });
        self.ovn.transact(&[set_cidr]);
        Ok(())
    }

    pub fn get_cidr(&self) -> Option<String> {
        self.other_config.get("subnet").map(|s| s.to_owned())
    }

    pub fn del_lsp(&mut self, lsp_id: &str) -> Result<(), Error> {
        let lsp = LogicalSwitchPort::get_by_name(self.ovn.clone(), lsp_id)?;

        let del_lsp = json!({
          "op": "mutate",
          "table": TYPE_LOGICAL_SWITCH,
          "mutations": [[
              "ports",
              "delete",
              [
                "set", [[ "uuid", lsp.uuid() ]]
              ]
          ]],
          "where": [[
              "_uuid", "==", ["uuid", self.uuid()]
          ]]
        });
        self.ovn.transact(&[del_lsp]);
        Ok(())
    }

    pub fn lsp(&mut self) -> LogicalSwitchPortBuilder {
        LogicalSwitchPortBuilder {
            ovn: self.ovn.clone(),
            ls: self,
        }
    }

    pub fn port_ids(&self) -> Vec<String> {
        self.port_ids.clone()
    }

    pub fn find_lsp_owner(ovn: Arc<Ovn>, lsp: &LogicalSwitchPort) -> Result<LogicalSwitch, Error> {
        let switches = LogicalSwitch::list(ovn)?;
        switches
            .into_iter()
            .find(|switch| switch.port_ids.contains(&lsp.uuid()))
            .ok_or_else(|| {
                Error::OvnNotFound(
                    LogicalSwitch::ovn_type(),
                    format!("owner of {}", lsp.uuid()),
                )
            })
    }
}

impl OvnCommon for LogicalSwitch {
    fn uuid(&self) -> String {
        self.uuid.clone()
    }

    fn ovn(&self) -> Arc<Ovn> {
        self.ovn.clone()
    }

    fn ovn_type() -> String {
        TYPE_LOGICAL_SWITCH.to_owned()
    }

    fn deserialize(ovn: Arc<Ovn>, value: &Value) -> Result<LogicalSwitch, Error> {
        let object = deserialize_object(value)?;

        Ok(LogicalSwitch {
            ovn,
            uuid: deserialize_uuid(object)?,
            name: deserialize_string(object, "name")?,
            port_ids: deserialize_uuid_set(object, "ports")?,
            other_config: deserialize_map(object, "other_config")?,
        })
    }
}

impl OvnNamed for LogicalSwitch {
    fn name(&self) -> String {
        self.name.to_owned()
    }
}

impl OvnBasicType for LogicalSwitch {}
