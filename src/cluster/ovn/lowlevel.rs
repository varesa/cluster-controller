use std::sync::Mutex;

use serde_json::{json, Map, Value};

use crate::cluster::ovn::jsonrpc::Params;
use crate::cluster::ovn::types::LogicalRouterStaticRoute;
use crate::Error;

use super::jsonrpc::{JsonRpcConnection, Message};

pub const TYPE_LOGICAL_SWITCH: &str = "Logical_Switch";
pub const TYPE_LOGICAL_SWITCH_PORT: &str = "Logical_Switch_Port";
pub const TYPE_LOGICAL_ROUTER: &str = "Logical_Router";
pub const TYPE_LOGICAL_ROUTER_PORT: &str = "Logical_Router_Port";
pub const TYPE_LOGICAL_ROUTER_STATIC_ROUTE: &str = "Logical_Router_Static_Route";
pub const TYPE_DHCP_OPTIONS: &str = "DHCP_Options";

pub struct Ovn {
    connection: Mutex<JsonRpcConnection>,
}

macro_rules! generate_list_fn {
    ($name:ident, $type:ident, $type_name:ident) => {
        pub fn $name(&self) -> Vec<$type> {
            let response = self.list_objects($type_name);
            let mut objects = Vec::new();
            for row in response {
                objects.push(serde_json::from_value(row).expect("deserialization failure"));
            }
            objects
        }
    };
}

impl Ovn {
    pub fn new(host: &str, port: u16) -> Self {
        Ovn {
            connection: Mutex::new(JsonRpcConnection::new(host, port)),
        }
    }

    pub(crate) fn transact(&self, operations: &[Value]) -> Vec<Value> {
        let mut params = vec![Value::String("OVN_Northbound".to_string())];
        params.append(&mut operations.to_owned());
        let response = self
            .connection
            .lock()
            .expect("Lock poisoned")
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

    pub fn list_objects(&self, object_type: &str) -> Vec<Value> {
        let columns = match object_type {
            TYPE_DHCP_OPTIONS => json!(["_uuid", "cidr"]),
            TYPE_LOGICAL_ROUTER => json!(["_uuid", "name", "static_routes"]),
            TYPE_LOGICAL_ROUTER_STATIC_ROUTE => json!(["_uuid", "ip_prefix", "nexthop"]),
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

    pub fn insert(&self, object_type: &str, params: Map<String, Value>) {
        let operation = json!({
            "op": "insert",
            "table": object_type,
            "row": params
        });
        self.transact(&[operation]);
    }

    pub fn delete_by_uuid(&self, object_type: &str, uuid: &str) {
        let operation = json!({
            "op": "delete",
            "table": object_type,
            "where": [
                ["_uuid", "==", ["uuid", uuid]]
            ]
        });
        self.transact(&[operation]);
    }

    generate_list_fn!(
        list_lr_static_routes,
        LogicalRouterStaticRoute,
        TYPE_LOGICAL_ROUTER_STATIC_ROUTE
    );

    pub fn get_lr_routes(&self, router_name: &str) -> Result<Vec<LogicalRouterStaticRoute>, Error> {
        let select = json!({
            "op": "select",
            "table": TYPE_LOGICAL_ROUTER,
            "where": [["name", "==", router_name]],
            "columns": ["static_routes"],
        });

        let rows = self.transact(&[select])[0]
            .as_object()
            .expect("Transaction result not an object")
            .get("rows")
            .expect("Transaction didn't return rows")
            .as_array()
            .expect("Rows is not an array")
            .to_owned();

        let router = rows
            .get(0)
            .ok_or_else(|| Error::OvnNotFound("LogicalRouter".into(), router_name.into()))?
            .as_object()
            .expect("Table row was not an object");

        let all_routes = self.list_lr_static_routes();

        let set = router
            .get("static_routes")
            .expect("Router doesn't have static_routes column")
            .as_array()
            .expect("static_routes was not an array")
            .get(1)
            .expect("static_routes was not in format ['set', [...]]");

        let uuids: Vec<String> = match set {
            Value::String(uuid) => vec![uuid.clone()],
            Value::Array(uuids_arrays) => uuids_arrays
                .iter()
                .map(|item| {
                    item.as_array()
                        .expect("Row was not an array like ['uuid', uuid]")[1]
                        .as_str()
                        .expect("UUID was not a string")
                        .to_string()
                })
                .collect(),
            _ => panic!("Unexpected data type"),
        };

        let routes = uuids
            .iter()
            .map(|uuid| {
                all_routes
                    .iter()
                    .find(|item| &item.uuid() == uuid)
                    .unwrap_or_else(|| {
                        panic!("Unable to find static route {} for {}", uuid, router_name)
                    })
                    .clone()
            })
            .collect::<Vec<LogicalRouterStaticRoute>>();
        Ok(routes)
    }
}
