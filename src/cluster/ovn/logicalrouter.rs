use std::sync::Arc;

use serde_json::{json, Value};

use crate::cluster::ovn::common::{OvnBasicType, OvnCommon, OvnGetters, OvnNamed, OvnNamedGetters};
use crate::cluster::ovn::deserialization::{
    deserialize_object, deserialize_string, deserialize_uuid,
};
use crate::cluster::ovn::logicalrouterport::LogicalRouterPort;
use crate::cluster::ovn::lowlevel::{
    Ovn, TYPE_LOGICAL_ROUTER, TYPE_LOGICAL_ROUTER_PORT, TYPE_LOGICAL_ROUTER_STATIC_ROUTE,
};
use crate::cluster::ovn::staticroute::StaticRoute;
use crate::crd::ovn::Route as RouteCrd;
use crate::Error;

pub struct LogicalRouter {
    ovn: Arc<Ovn>,
    uuid: String,
    name: String,
    static_route_uuids: Vec<String>,
}

impl LogicalRouter {
    pub fn add_lrp(&mut self, lrp_name: &str, networks: &str) -> Result<LogicalRouterPort, Error> {
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
                ["_uuid", "==", ["uuid", self.uuid()]]
            ],
            "mutations": [
                ["ports", "insert", ["set", [["named-uuid", "new_lrp"]]]]
            ]
        });
        self.ovn.transact(&[add_lrp, add_lrp_to_lr]);
        LogicalRouterPort::get_by_name(self.ovn.clone(), lrp_name)
    }

    pub fn set_routes(&mut self, new_routes: &[RouteCrd]) -> Result<(), Error> {
        let old_routes = self.get_routes()?;

        let mut operations = Vec::new();
        let mut to_add = Vec::new();
        let mut to_remove = Vec::new();

        let mut i = 0u8;

        for new_route in new_routes {
            if !old_routes.iter().any(|old_route| {
                old_route.ip_prefix == new_route.cidr && old_route.nexthop == new_route.nexthop
            }) {
                let id = format!("new_route_{}", i);
                i += 1;
                let create_op = json!({
                    "op": "insert",
                    "table": TYPE_LOGICAL_ROUTER_STATIC_ROUTE,
                    "row": {"ip_prefix": new_route.cidr, "nexthop": new_route.nexthop},
                    "uuid-name": id
                });
                operations.push(create_op);
                to_add.push(json!(["named-uuid", id]));
            }
        }
        for old_route in old_routes {
            if !new_routes.iter().any(|new_route| {
                new_route.cidr == old_route.ip_prefix && new_route.nexthop == old_route.nexthop
            }) {
                to_remove.push(json!(["uuid", old_route.uuid()]));
            }
        }

        let update = json!({
            "mutations":[
                ["static_routes","insert",["set", to_add]],
                ["static_routes","delete",["set", to_remove]]
            ],
            "where":[[
                "_uuid","==",["uuid",self.uuid()]
            ]],
            "op":"mutate","table":"Logical_Router"
        });
        operations.push(update);

        self.ovn.transact(&operations);
        Ok(())
    }

    pub fn get_routes(&self) -> Result<Vec<StaticRoute>, Error> {
        let all_routes = StaticRoute::list(self.ovn.clone())?;
        let routes = self
            .static_route_uuids
            .iter()
            .map(|uuid| {
                all_routes
                    .iter()
                    .find(|item| &item.uuid() == uuid)
                    .unwrap_or_else(|| {
                        panic!("Unable to find static route {} for {}", uuid, self.name())
                    })
                    .clone()
            })
            .collect::<Vec<StaticRoute>>();
        Ok(routes)
    }
}

impl OvnCommon for LogicalRouter {
    fn uuid(&self) -> String {
        self.uuid.clone()
    }

    fn ovn(&self) -> Arc<Ovn> {
        self.ovn.clone()
    }

    fn ovn_type() -> String {
        TYPE_LOGICAL_ROUTER.to_owned()
    }

    fn deserialize(ovn: Arc<Ovn>, value: &Value) -> Result<LogicalRouter, Error> {
        let object = deserialize_object(value)?;

        let route_set = object
            .get("static_routes")
            .and_then(|a| a.as_array())
            .and_then(|a| a.get(1))
            .ok_or(Error::OvnDeserializationFailed)?;
        let route_uuids: Vec<String> = match route_set {
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

        Ok(LogicalRouter {
            ovn,
            uuid: deserialize_uuid(object)?,
            name: deserialize_string(object, "name")?,
            static_route_uuids: route_uuids,
        })
    }
}

impl OvnNamed for LogicalRouter {
    fn name(&self) -> String {
        self.name.to_owned()
    }
}

impl OvnBasicType for LogicalRouter {}
