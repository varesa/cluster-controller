use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::cluster::ovn::common::{OvnBasicType, OvnCommon, OvnNamed};
use crate::cluster::ovn::lowlevel::{Ovn, TYPE_LOGICAL_SWITCH, TYPE_LOGICAL_SWITCH_PORT};
use crate::Error;

pub struct LogicalSwitch {
    ovn: Arc<Ovn>,
    uuid: String,
    name: String,
}

macro_rules! try_deserialize {
    ($e:expr) => {
        $e.ok_or_else(|| Error::OvnDeserializationFailed)?
    };
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
    ) -> Result<(), Error> {
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
        Ok(())
    }

    pub fn del_lsp(&mut self, lsp_id: &str) -> Result<(), Error> {
        let lsp = self.ovn.get_lsp(lsp_id)?;

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
        let object = try_deserialize!(value.as_object());

        Ok(LogicalSwitch {
            ovn,
            uuid: try_deserialize!(object
                .get("_uuid")
                .and_then(|a| a.as_array())
                .and_then(|a| a.get(0))
                .and_then(|u| u.as_str()))
            .to_owned(),
            name: try_deserialize!(object.get("name").and_then(|u| u.as_str())).to_owned(),
        })
    }
}

impl OvnNamed for LogicalSwitch {
    fn name(&self) -> String {
        self.name.to_owned()
    }
}

impl OvnBasicType for LogicalSwitch {}
