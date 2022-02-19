use super::jsonrpc::{JsonRpcConnection, Message};
use crate::cluster::ovn::jsonrpc::Params;
use crate::cluster::ovn::types::LogicalSwitch;
use serde_json::{json, Map, Value};

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
        let response = self.connection.request(
            "monitor_cond_since",
            &Params::from_json(json!([
                "OVN_Northbound",
                ["monid", "OVN_Northbound"],
                {
                    object_type: [{"columns": ["name"]}]
                },
                "00000000-0000-0000-0000-000000000000"
            ])),
        );
        match response {
            Message::Response { error, result, .. } => {
                assert!(error.is_null());
                result[2][object_type].clone()
            }
            _ => panic!("Didn't get response"),
        }
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

    pub fn add_ls(&mut self, name: &str) {
        let mut params = Map::new();
        params.insert("name".to_string(), Value::String(name.to_string()));
        self.insert("Logical_Switch", params);
    }

    pub fn list_ls(&mut self) -> Vec<LogicalSwitch> {
        let response = self.list_objects("Logical_Switch");
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
}
