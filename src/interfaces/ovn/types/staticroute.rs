use std::sync::Arc;

use serde_json::Value;

use crate::Error;
use crate::interfaces::ovn::common::OvnCommon;
use crate::interfaces::ovn::deserialization::{
    deserialize_object, deserialize_string, deserialize_uuid,
};
use crate::interfaces::ovn::lowlevel::{Ovn, TYPE_LOGICAL_ROUTER_STATIC_ROUTE};

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
        let object = deserialize_object(value)?;

        Ok(StaticRoute {
            ovn,
            uuid: deserialize_uuid(object)?,
            ip_prefix: deserialize_string(object, "ip_prefix")?,
            nexthop: deserialize_string(object, "nexthop")?,
        })
    }
}
