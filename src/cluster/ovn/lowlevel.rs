use std::net::IpAddr;

use super::jsonrpc::{JsonRpcConnection, Message};
use crate::cluster::ovn::jsonrpc::Params;
use crate::cluster::ovn::types::{
    DhcpOptions, LogicalRouter, LogicalRouterPort, LogicalRouterStaticRoute, LogicalSwitch,
    LogicalSwitchPort,
};
use crate::crd::ovn::DhcpOptions as DhcpOptionsCrd;
use crate::crd::ovn::Route as RouteCrd;
use crate::Error;
use ipnet::IpNet;
use serde_json::{json, Map, Value};

const TYPE_LOGICAL_SWITCH: &str = "Logical_Switch";
const TYPE_LOGICAL_SWITCH_PORT: &str = "Logical_Switch_Port";
const TYPE_LOGICAL_ROUTER: &str = "Logical_Router";
const TYPE_LOGICAL_ROUTER_PORT: &str = "Logical_Router_Port";
const TYPE_LOGICAL_ROUTER_STATIC_ROUTE: &str = "Logical_Router_Static_Route";
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
                objects.push(serde_json::from_value(row).expect("deserialization failure"));
            }
            objects
        }
    };
}

macro_rules! generate_get_fn {
    ($name:ident, $type:ident, $type_name:ident, $list_fn:ident) => {
        pub fn $name(&mut self, name: &str) -> Result<$type, Error> {
            let objects = self.$list_fn();
            objects
                .into_iter()
                .find(|o| o.name == name)
                .ok_or_else(|| Error::OvnNotFound($type_name.to_string(), name.to_string()))
        }
    };
}

macro_rules! generate_delete_fn {
    ($name:ident, $type:ident, $type_name:ident, $get_fn:ident) => {
        pub fn $name(&mut self, name: &str) -> Result<(), Error> {
            let object = self.$get_fn(name)?;
            self.delete_by_uuid($type_name, &object.uuid());
            Ok(())
        }
    };
}

macro_rules! generate_add_fn {
    ($name:ident, $type_name:ident) => {
        pub fn $name(&mut self, name: &str) {
            let mut params = Map::new();
            params.insert("name".to_string(), Value::String(name.to_string()));
            self.insert($type_name, params);
        }
    };
}

macro_rules! generate_all_fn {
    ($type_name:ident, $type:ident, $add_fn:ident, $list_fn:ident, $get_fn:ident, $delete_fn:ident) => {
        generate_add_fn!($add_fn, $type_name);
        generate_list_fn!($list_fn, $type, $type_name);
        generate_get_fn!($get_fn, $type, $type_name, $list_fn);
        generate_delete_fn!($delete_fn, $type, $type_name, $get_fn);
    };
}

impl Ovn {
    pub fn new(host: &str, port: u16) -> Self {
        Ovn {
            connection: JsonRpcConnection::new(host, port),
        }
    }

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

    generate_all_fn!(
        TYPE_LOGICAL_SWITCH,
        LogicalSwitch,
        add_ls,
        list_ls,
        get_ls,
        del_ls_by_name
    );
    generate_all_fn!(
        TYPE_LOGICAL_ROUTER,
        LogicalRouter,
        add_lr,
        list_lr,
        get_lr,
        del_lr_by_name
    );

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

    pub fn get_lr_static_route(
        &mut self,
        prefix: &str,
        nexthop: &str,
    ) -> Result<LogicalRouterStaticRoute, Error> {
        let routes = self.list_lr_static_routes();
        routes
            .into_iter()
            .find(|route| route.ip_prefix == prefix && route.nexthop == nexthop)
            .ok_or_else(|| {
                Error::OvnNotFound(
                    "LogicalRouterStaticRoute".into(),
                    format!("{} via {}", prefix, nexthop),
                )
            })
    }

    pub fn add_lsp(
        &mut self,
        ls_name: &str,
        lsp_name: &str,
        extra_params: Option<&Map<String, Value>>,
    ) -> Result<(), Error> {
        let ls = self.get_ls(ls_name)?;

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
                ["_uuid", "==", ["uuid", ls.uuid()]]
            ],
            "mutations": [
                ["ports", "insert", ["set", [["named-uuid", "new_lsp"]]]]
            ]
        });
        self.transact(&[add_lsp, add_lsp_to_ls]);
        Ok(())
    }

    pub fn add_lrp(&mut self, lr_name: &str, lrp_name: &str, networks: &str) -> Result<(), Error> {
        let lr = self.get_lr(lr_name)?;

        let add_lrp = json!({
            "op": "insert",
            "table": TYPE_LOGICAL_ROUTER_PORT,
            "row": {
                "name": lrp_name,
                "mac": "02:00:00:00:00:01",
                "networks": networks,
            },
            "uuid-name": "new_lrp"
        });
        let add_lrp_to_lr = json!({
            "op": "mutate",
            "table": TYPE_LOGICAL_ROUTER,
            "where": [
                ["_uuid", "==", ["uuid", lr.uuid()]]
            ],
            "mutations": [
                ["ports", "insert", ["set", [["named-uuid", "new_lrp"]]]]
            ]
        });
        self.transact(&[add_lrp, add_lrp_to_lr]);
        Ok(())
    }

    pub fn del_lsp(&mut self, ls_name: &str, lsp_id: &str) -> Result<(), Error> {
        let ls = self.get_ls(ls_name)?;

        let lsp = self.get_lsp(lsp_id)?;

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
        let lsp = self.get_lsp(lsp_id)?;

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
        let create_options = json!({
            "op": "insert",
            "table": "DHCP_Options",
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
            "table": "DHCP_Options",
            "where": [["_uuid", "==", ["uuid", option_set.uuid()]]],
            "row": {"options": ["map", options]}
        });
        self.transact(&[set_options]);
        Ok(())
    }

    pub fn create_lr_static_route(&mut self, route: &RouteCrd) -> Result<(), Error> {
        let prefix = route.cidr.clone();
        let nexthop = route.nexthop.clone();
        let create_options = json!({
            "op": "insert",
            "table": "DHCP_Options",
            "row": {"ip_prefix": prefix, "nexthop": nexthop},
            "uuid-name": "new_dhcp_options"
        });
        self.transact(&[create_options]);
        Ok(())
    }

    pub fn get_lr_routes(
        &mut self,
        router_name: &str,
    ) -> Result<Vec<LogicalRouterStaticRoute>, Error> {
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
        let routes = router
            .get("static_routes")
            .expect("Router doesn't have static_routes column")
            .as_array()
            .expect("static_routes was not an array")
            .get(1)
            .expect("static_routes was not in format ['set', [...]]")
            .as_array()
            .expect("set not followed by an array")
            .iter()
            .map(|route| {
                let route = route
                    .as_array()
                    .expect("Route was not in format ['uuid', '...']");
                let uuid = route
                    .get(1)
                    .expect("Route didn't have an UUID")
                    .as_str()
                    .expect("UUID was not a string");
                all_routes
                    .iter()
                    .find(|item| item._uuid[0] == uuid)
                    .unwrap_or_else(|| {
                        panic!("Unable to find static route {} for {}", uuid, router_name)
                    })
                    .clone()
            })
            .collect::<Vec<LogicalRouterStaticRoute>>();
        Ok(routes)
    }

    pub fn set_lr_routes(
        &mut self,
        router_name: &str,
        new_routes: &[RouteCrd],
    ) -> Result<(), Error> {
        let old_routes = self.get_lr_routes(router_name)?;

        let mut to_add = Vec::new();
        let mut to_remove = Vec::new();

        for new_route in new_routes {
            if !old_routes.iter().any(|old_route| {
                old_route.ip_prefix == new_route.cidr && old_route.nexthop == new_route.nexthop
            }) {
                let uuid = self
                    .get_lr_static_route(&new_route.cidr, &new_route.nexthop)
                    .expect("Route missing")
                    ._uuid[0]
                    .clone();
                to_add.push(json!(["uuid", uuid]));
            }
        }
        for old_route in old_routes {
            if !new_routes.iter().any(|new_route| {
                new_route.cidr == old_route.ip_prefix && new_route.nexthop == old_route.nexthop
            }) {
                to_remove.push(json!(["uuid", old_route._uuid[0]]));
            }
        }

        let update = json!({
            "mutations":[
                ["static_routes","insert",["set",[to_add]]],
                ["static_routes","delete",["set",[["named-uuid","row15bc27fc_382b_4e87_a176_08445c64cbcb"]]]]
            ],
            "where":[[
                "_uuid","==",["uuid","f1f3d500-b398-4976-ae25-bbaac0fe8125"]
            ]],
            "op":"mutate","table":"Logical_Router"
        });
        self.transact(&[update]);

        Ok(())
    }

    pub fn set_ls_cidr(&mut self, ls_name: &str, cidr: &str) -> Result<(), Error> {
        let ls = self.get_ls(ls_name)?;

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
