use std::net::IpAddr;
use std::sync::Mutex;

use ipnet::IpNet;
use serde_json::{json, Map, Value};

use crate::cluster::ovn::jsonrpc::Params;
use crate::cluster::ovn::types::{
    DhcpOptions, LogicalRouterPort, LogicalRouterStaticRoute, LogicalSwitchPort,
};
use crate::crd::ovn::DhcpOptions as DhcpOptionsCrd;
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

macro_rules! generate_get_fn {
    ($name:ident, $type:ident, $type_name:ident, $list_fn:ident) => {
        pub fn $name(&self, name: &str) -> Result<$type, Error> {
            let objects = self.$list_fn();
            objects
                .into_iter()
                .find(|o| o.name == name)
                .ok_or_else(|| Error::OvnNotFound($type_name.to_string(), name.to_string()))
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

    generate_list_fn!(list_lsp, LogicalSwitchPort, TYPE_LOGICAL_SWITCH_PORT);
    generate_get_fn!(
        get_lsp,
        LogicalSwitchPort,
        TYPE_LOGICAL_SWITCH_PORT,
        list_lsp
    );

    generate_list_fn!(list_lrp, LogicalRouterPort, TYPE_LOGICAL_ROUTER_PORT);
    generate_get_fn!(
        get_lrp,
        LogicalRouterPort,
        TYPE_LOGICAL_ROUTER_PORT,
        list_lrp
    );

    generate_list_fn!(list_dhcp_options, DhcpOptions, TYPE_DHCP_OPTIONS);
    generate_list_fn!(
        list_lr_static_routes,
        LogicalRouterStaticRoute,
        TYPE_LOGICAL_ROUTER_STATIC_ROUTE
    );

    pub fn get_dhcp_options(&mut self, cidr: &str) -> Option<DhcpOptions> {
        let option_sets = self.list_dhcp_options();
        option_sets
            .into_iter()
            .find(|option_set| option_set.cidr == cidr)
    }

    pub fn update_lrp(&self, lrp_name: &str, networks: &str) -> Result<(), Error> {
        let lrp = self.get_lrp(lrp_name)?;

        let update_lrp = json!({
            "op": "update",
            "table": TYPE_LOGICAL_ROUTER_PORT,
            "where": [
                ["_uuid", "==", ["uuid", lrp.uuid()]]
            ],
            "row": {
                "name": lrp_name,
                "mac": "02:00:00:00:00:01",
                "networks": networks,
            }
        });
        self.transact(&[update_lrp]);
        Ok(())
    }

    pub fn set_lsp_address(&mut self, lsp_id: &str, mac_address: &str) -> Result<(), Error> {
        let lsp = self.get_lsp(lsp_id)?;

        let set_address = json!({
            "op": "update",
            "table": TYPE_LOGICAL_SWITCH_PORT,
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
        let create_options = json!({
            "op": "insert",
            "table": TYPE_DHCP_OPTIONS,
            "row": {"cidr": cidr},
            "uuid-name": "new_dhcp_options"
        });
        self.transact(&[create_options]);
        Ok(())
    }

    pub fn set_dhcp_option_set_options(
        &mut self,
        dhcp_options: &DhcpOptionsCrd,
    ) -> Result<(), Error> {
        let cidr = dhcp_options.cidr.clone();
        let option_set = self
            .get_dhcp_options(&cidr)
            .ok_or_else(|| Error::OvnNotFound(TYPE_DHCP_OPTIONS.to_string(), cidr.to_string()))?;

        let net: IpNet = cidr.parse()?;
        let hosts: Vec<IpAddr> = net.hosts().collect();
        let mut options = vec![
            [String::from("server_id"), hosts[1].to_string()],
            [
                String::from("server_mac"),
                String::from("c0:ff:ee:00:00:01"),
            ],
        ];

        // Copy values from CRD to above vector
        macro_rules! push_dhcp_opts {
            ($source:ident, $destination:ident, [$($name:ident),+]) => {
                $(
                if let Some(value) = $source.$name.clone() {
                    $destination.push([String::from(stringify!($name)), value.to_string()]);
                }
                )+
            }
        }
        push_dhcp_opts!(
            dhcp_options,
            options,
            [lease_time, dns_server, domain_name, router]
        );

        let set_options = json!({
            "op": "update",
            "table": TYPE_DHCP_OPTIONS,
            "where": [["_uuid", "==", ["uuid", option_set.uuid()]]],
            "row": {"options": ["map", options]}
        });
        self.transact(&[set_options]);
        Ok(())
    }

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

    pub fn set_lsp_dhcp_options(&mut self, lsp_id: &str, cidr: &str) -> Result<(), Error> {
        let lsp = self.get_lsp(lsp_id)?;

        let dhcp_options = self
            .get_dhcp_options(cidr)
            .ok_or_else(|| Error::OvnNotFound(TYPE_DHCP_OPTIONS.to_string(), cidr.to_string()))?;

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
