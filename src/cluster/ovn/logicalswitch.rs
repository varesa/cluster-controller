use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::cluster::ovn::common::{OvnBasicType, OvnCommon, OvnNamed, OvnNamedGetters};
use crate::cluster::ovn::deserialization::{
    deserialize_object, deserialize_string, deserialize_uuid,
};
use crate::cluster::ovn::logicalswitchport::LogicalSwitchPort;
use crate::cluster::ovn::lowlevel::{Ovn, TYPE_LOGICAL_SWITCH, TYPE_LOGICAL_SWITCH_PORT};
use crate::Error;

pub struct LogicalSwitch {
    ovn: Arc<Ovn>,
    uuid: String,
    name: String,
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

    pub fn add_lsp(
        &mut self,
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
                ["_uuid", "==", ["uuid", self.uuid()]]
            ],
            "mutations": [
                ["ports", "insert", ["set", [["named-uuid", "new_lsp"]]]]
            ]
        });
        self.ovn.transact(&[add_lsp, add_lsp_to_ls]);
        LogicalSwitchPort::get_by_name(self.ovn.clone(), lsp_name)
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
        })
    }
}

impl OvnNamed for LogicalSwitch {
    fn name(&self) -> String {
        self.name.to_owned()
    }
}

impl OvnBasicType for LogicalSwitch {}
