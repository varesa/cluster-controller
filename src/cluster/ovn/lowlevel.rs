use super::jsonrpc::{JsonRpcConnection, Message};
use crate::cluster::ovn::jsonrpc::Params;
use crate::cluster::ovn::types::{DhcpOptions, LogicalSwitch, LogicalSwitchPort};
use crate::crd::ovn::DhcpOptions as DhcpOptionsCrd;
use crate::Error;
use serde_json::{json, Map, Value};

const TYPE_LOGICAL_SWITCH: &str = "Logical_Switch";
const TYPE_LOGICAL_SWITCH_PORT: &str = "Logical_Switch_Port";
const TYPE_DHCP_OPTIONS: &str = "DHCP_Options";

pub struct Ovn {
    connection: JsonRpcConnection,
}

macro_rules! generate_list_fn {
    ($name:ident, $type:ident, $type_name:ident) => {
        pub fn $name(&mut self) -> Vec<$type> {
            let response = self.list_objects($type_name);
            let mut objects = Vec::new();
            for row in response {
                objects.push(serde_json::from_value(row).expect("LSP deserialization failure"));
            }
            objects
        }
    };
}

fn split_cidr(cidr: &str) -> (String, String) {
    let mut split = cidr.split('/');
    let prefix = split.next().unwrap().to_string();
    let length = split.next().unwrap().to_string();
    (prefix, length)
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

    fn transact(&mut self, operations: &[Value]) -> Vec<Value> {
        let mut params = vec![Value::String("OVN_Northbound".to_string())];
        params.append(&mut operations.to_owned());
        let response = self
            .connection
            .request("transact", &Params::from_json(json!(params)));
        match response {
            Message::Response { error, result, .. } => {
                assert!(error.is_null());
                let results = result.as_array().expect("result not an array").to_owned();
                for result in results.iter() {
                    let error = result
                        .as_object()
                        .expect("result should be an object")
                        .get("error");
                    assert!(error.is_none() || error.unwrap().is_null());
                }
                results
            }
            _ => panic!("Didn't get response"),
        }
    }

    fn list_objects(&mut self, object_type: &str) -> Vec<Value> {
        let columns = match object_type {
            TYPE_DHCP_OPTIONS => json!(["_uuid", "cidr"]),
            _ => json!(["_uuid", "name"]),
        };
        let select = json!({
            "op": "select",
            "table": object_type,
            "where": [],
            "columns": columns
        });
        self.transact(&[select])[0]
            .as_object()
            .expect("Transaction result not an object")
            .get("rows")
            .expect("Transaction didn't return rows")
            .as_array()
            .expect("Rows is not an array")
            .to_owned()
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

    generate_list_fn!(list_ls, LogicalSwitch, TYPE_LOGICAL_SWITCH);
    generate_list_fn!(list_lsp, LogicalSwitchPort, TYPE_LOGICAL_SWITCH_PORT);
    generate_list_fn!(list_dhcp_options, DhcpOptions, TYPE_DHCP_OPTIONS);

    pub fn get_ls(&mut self, name: &str) -> Option<LogicalSwitch> {
        let switches = self.list_ls();
        switches.into_iter().find(|sw| sw.name == name)
    }

    pub fn get_lsp(&mut self, name: &str) -> Option<LogicalSwitchPort> {
        let ports = self.list_lsp();
        ports.into_iter().find(|lsp| lsp.name == name)
    }

    pub fn get_dhcp_options(&mut self, cidr: &str) -> Option<DhcpOptions> {
        let option_sets = self.list_dhcp_options();
        option_sets
            .into_iter()
            .find(|option_set| option_set.cidr == cidr)
    }

    pub fn del_ls_by_name(&mut self, name: &str) -> Result<(), Error> {
        let ls = self
            .get_ls(name)
            .ok_or_else(|| Error::SwitchNotFound(name.to_string()))?;
        self.delete_by_uuid(TYPE_LOGICAL_SWITCH, &ls.uuid());
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
            "uuid-name": "new_lsp"
        });
        let add_lsp_to_ls = json!({
            "op": "mutate",
            "table": TYPE_LOGICAL_SWITCH,
            "where": [
                ["_uuid", "==", ["uuid", ls.uuid()]]
            ],
            "mutations": [
                ["ports", "insert", ["set", [["named-uuid", "new_lsp"]]]]
            ]
        });
        self.transact(&[add_lsp, add_lsp_to_ls]);
        Ok(())
    }

    pub fn del_lsp(&mut self, ls_name: &str, lsp_id: &str) -> Result<(), Error> {
        let ls = self
            .get_ls(ls_name)
            .ok_or_else(|| Error::SwitchNotFound(ls_name.to_string()))?;

        let lsp = self
            .get_lsp(lsp_id)
            .ok_or_else(|| Error::SwitchPortNotFound(lsp_id.to_string()))?;

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
              "_uuid", "==", ["uuid", ls.uuid()]
          ]]
        });
        self.transact(&[del_lsp]);
        Ok(())
    }

    pub fn set_lsp_address(&mut self, lsp_id: &str, mac_address: &str) -> Result<(), Error> {
        let lsp = self
            .get_lsp(lsp_id)
            .ok_or_else(|| Error::SwitchPortNotFound(lsp_id.to_string()))?;

        let set_address = json!({
            "op": "update",
            "table": "Logical_Switch_Port",
            "where": [
                [ "_uuid", "==", [ "uuid", lsp.uuid() ] ]
            ],
            "row": { "addresses": format!("{mac_address} dynamic") }
        });
        self.transact(&[set_address]);
        Ok(())
    }

    pub fn create_dhcp_option_set(&mut self, dhcp_options: &DhcpOptionsCrd) -> Result<(), Error> {
        let cidr = dhcp_options.cidr.clone();
        let (prefix, _prefix_length) = split_cidr(&cidr);
        let create_options = json!({
            "op": "insert",
            "table": "DHCP_Options",
            "row": {"cidr": cidr},
            "uuid-name": "new_dhcp_options"
        });

        let options = json!([
            ["server_id", format!("{prefix}1")],
            ["server_mac", "c0:ff:ee:00:00:01"],
            ["lease_time", "3600"]
        ]);

        let set_options = json!({
            "op": "update",
            "table": "DHCP_Options",
            "where": [["_uuid", "==", ["named-uuid", "new_dhcp_options"]]],
            "row": {"options": ["map", options]}
        });
        self.transact(&[create_options, set_options]);
        Ok(())
    }

    pub fn set_ls_cidr(&mut self, ls_name: &str, cidr: &str) -> Result<(), Error> {
        let ls = self
            .get_ls(ls_name)
            .ok_or_else(|| Error::SwitchNotFound(ls_name.to_string()))?;

        let set_cidr = json!({
            "op": "update",
            "table": "Logical_Switch",
            "where": [["_uuid", "==", ["uuid", ls.uuid()]]],
            "row": {"other_config": ["map", [["subnet", cidr]]]}
        });
        self.transact(&[set_cidr]);
        Ok(())
    }

    pub fn set_lsp_dhcp_options(&mut self, lsp_id: &str, cidr: &str) -> Result<(), Error> {
        let lsp = self
            .get_lsp(lsp_id)
            .ok_or_else(|| Error::SwitchPortNotFound(lsp_id.to_string()))?;

        let dhcp_options = self
            .get_dhcp_options(cidr)
            .ok_or_else(|| Error::DhcpOptionsNotFound(cidr.to_string()))?;

        let set_dhcp_options = json!({
            "op": "update",
            "table": "Logical_Switch_Port",
            "where": [["_uuid", "==", ["uuid", lsp.uuid()]]],
            "row": {"dhcpv4_options":["uuid", dhcp_options.uuid()]}
        });

        self.transact(&[set_dhcp_options]);
        Ok(())
    }
}
