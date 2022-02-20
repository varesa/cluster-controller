use super::jsonrpc::{JsonRpcConnection, Message};
use crate::cluster::ovn::jsonrpc::Params;
use crate::cluster::ovn::types::{LogicalSwitch, LogicalSwitchPort};
use crate::Error;
use serde_json::{json, Map, Value};

const TYPE_LOGICAL_SWITCH: &str = "Logical_Switch";
const TYPE_LOGICAL_SWITCH_PORT: &str = "Logical_Switch_Port";

pub struct Ovn {
    connection: JsonRpcConnection,
}

impl Ovn {
    pub fn new(host: &str, port: u16) -> Self {
        Ovn {
            connection: JsonRpcConnection::new(host, port),
        }
    }

    /*pub fn echo(&mut self) {
        let echo = self.connection.request("echo", Some(json!([])));
        assert!(echo.error.is_null());
    }

    #[allow(dead_code)]
    pub fn print_schema(&mut self) {
        let schema = self
            .connection
            .request("get_schema", Some(json!(["OVN_Northbound"])));
        print!("{schema:#?}");
    }*/

    fn list_objects(&mut self, object_type: &str) -> Value {
        let params = json!([
            "OVN_Northbound",
            ["monid", "OVN_Northbound"],
            {
                object_type: [{"columns": ["name"]}]
            },
            "00000000-0000-0000-0000-000000000000"
        ]);

        let response = self
            .connection
            .request("monitor_cond_since", &Params::from_json(params.clone()));
        let objects = match response {
            Message::Response { error, result, .. } => {
                assert!(error.is_null());
                if result[2].as_object().unwrap().contains_key(object_type) {
                    result[2][object_type].clone()
                } else {
                    Value::Object(Map::new())
                }
            }
            _ => panic!("Didn't get response"),
        };
        // Cancel the monitor
        self.connection
            .request("monitor_cancel", &Params::from_json(params));
        objects
    }

    fn transact(&mut self, operations: &[Value]) {
        let mut params = vec![Value::String("OVN_Northbound".to_string())];
        params.append(&mut operations.to_owned());
        let response = self
            .connection
            .request("transact", &Params::from_json(json!(params)));
        match response {
            Message::Response { error, .. } => {
                assert!(error.is_null());
            }
            _ => panic!("Didn't get response"),
        }
    }

    fn insert(&mut self, object_type: &str, params: Map<String, Value>) {
        let operation = json!({
            "op": "insert",
            "table": object_type,
            "row": params
        });
        self.transact(&[operation]);
    }

    fn delete_by_uuid(&mut self, object_type: &str, uuid: &str) {
        let operation = json!({
            "op": "delete",
            "table": object_type,
            "where": [
                ["_uuid", "==", ["uuid", uuid]]
            ]
        });
        self.transact(&[operation]);
    }

    pub fn add_ls(&mut self, name: &str) {
        let mut params = Map::new();
        params.insert("name".to_string(), Value::String(name.to_string()));
        self.insert(TYPE_LOGICAL_SWITCH, params);
    }

    pub fn list_ls(&mut self) -> Vec<LogicalSwitch> {
        let response = self.list_objects(TYPE_LOGICAL_SWITCH);
        let mut switches = Vec::new();
        for (uuid, params) in response.as_object().unwrap().iter() {
            switches.push(LogicalSwitch::from_json(
                uuid,
                params
                    .as_object()
                    .expect("Should be object")
                    .get("initial")
                    .expect("Should contain initial key")
                    .as_object()
                    .expect("asd"),
            ));
        }
        switches
    }

    pub fn list_lsp(&mut self) -> Vec<LogicalSwitchPort> {
        let response = self.list_objects(TYPE_LOGICAL_SWITCH_PORT);
        let mut ports = Vec::new();
        for (uuid, params) in response.as_object().unwrap().iter() {
            ports.push(LogicalSwitchPort::from_json(
                uuid,
                params
                    .as_object()
                    .expect("Should be object")
                    .get("initial")
                    .expect("Should contain initial key")
                    .as_object()
                    .expect("asd"),
            ));
        }
        ports
    }

    pub fn get_ls(&mut self, name: &str) -> Option<LogicalSwitch> {
        let switches = self.list_ls();
        switches.into_iter().find(|sw| sw.name == name)
    }

    pub fn get_lsp(&mut self, name: &str) -> Option<LogicalSwitchPort> {
        let ports = self.list_lsp();
        ports.into_iter().find(|lsp| lsp.name == name)
    }

    pub fn del_ls_by_name(&mut self, name: &str) -> Result<(), Error> {
        let ls = self
            .get_ls(name)
            .ok_or_else(|| Error::SwitchNotFound(name.to_string()))?;
        self.delete_by_uuid(TYPE_LOGICAL_SWITCH, &ls.uuid);
        Ok(())
    }

    pub fn add_lsp(&mut self, ls_name: &str, lsp_id: &str) -> Result<(), Error> {
        let ls = self
            .get_ls(ls_name)
            .ok_or_else(|| Error::SwitchNotFound(ls_name.to_string()))?;

        let add_lsp = json!({
            "op": "insert",
            "table": TYPE_LOGICAL_SWITCH_PORT,
            "row": {
                "name": lsp_id
            },
            "uuid-name": "new-lsp"
        });
        let add_lsp_to_ls = json!({
            "op": "mutate",
            "table": TYPE_LOGICAL_SWITCH,
            "where": [
                ["_uuid", "==", ls.uuid]
            ],
            "mutations": [
                ["ports", "insert", ["set", [["named-uuid", "new-lsp"]]]]
            ]
        });
        self.transact(&[add_lsp, add_lsp_to_ls]);

        Ok(())
    }
}
