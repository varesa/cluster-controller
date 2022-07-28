use std::sync::Arc;

use serde_json::Value;

use crate::cluster::ovn::common::OvnCommon;
use crate::cluster::ovn::lowlevel::{Ovn, TYPE_LOGICAL_ROUTER_STATIC_ROUTE};
use crate::{try_deserialize, Error};

#[derive(Clone)]
pub struct StaticRoute {
    ovn: Arc<Ovn>,
    uuid: String,
    pub ip_prefix: String,
    pub nexthop: String,
}

impl StaticRoute {}

impl OvnCommon for StaticRoute {
    fn uuid(&self) -> String {
        self.uuid.clone()
    }

    fn ovn(&self) -> Arc<Ovn> {
        self.ovn.clone()
    }

    fn ovn_type() -> String {
        TYPE_LOGICAL_ROUTER_STATIC_ROUTE.to_owned()
    }

    fn deserialize(ovn: Arc<Ovn>, value: &Value) -> Result<Self, Error> {
        let object = try_deserialize!(value.as_object());

        Ok(StaticRoute {
            ovn,
            uuid: try_deserialize!(object
                .get("_uuid")
                .and_then(|a| a.as_array())
                .and_then(|a| a.get(1))
                .and_then(|u| u.as_str()))
            .to_owned(),
            ip_prefix: try_deserialize!(object.get("ip_prefix").and_then(|u| u.as_str()))
                .to_owned(),
            nexthop: try_deserialize!(object.get("nexthop").and_then(|u| u.as_str())).to_owned(),
        })
    }
}
