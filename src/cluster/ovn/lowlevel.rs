use super::jsonrpc::JsonRpcConnection;
use crate::cluster::ovn::types::LogicalSwitch;
use serde_json::{json, Value};

pub struct Ovn {
    connection: JsonRpcConnection,
}

impl Ovn {
    pub fn new(host: &str, port: u16) -> Self {
        Ovn {
            connection: JsonRpcConnection::new(host, port),
        }
    }

    pub fn echo(&mut self) {
        let echo = self.connection.request("echo", Some(json!([])));
        assert!(echo.error.is_null());
    }

    #[allow(dead_code)]
    pub fn print_schema(&mut self) {
        let schema = self
            .connection
            .request("get_schema", Some(json!(["OVN_Northbound"])));
        print!("{schema:#?}");
    }

    fn list_objects(&mut self, object_type: &str) -> Value {
        let response = self.connection.request(
            "monitor_cond_since",
            Some(json!([
                "OVN_Northbound",
                ["monid", "OVN_Northbound"],
                {
                    object_type: [{"columns": ["name"]}]
                },
                "00000000-0000-0000-0000-000000000000"
            ])),
        );
        assert!(response.error.is_null());
        response.result[2][object_type].clone()
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
