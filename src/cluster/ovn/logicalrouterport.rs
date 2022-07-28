use std::sync::Arc;

use serde_json::{json, Value};

use crate::cluster::ovn::common::{OvnCommon, OvnNamed};
use crate::cluster::ovn::deserialization::{
    deserialize_object, deserialize_string, deserialize_uuid,
};
use crate::cluster::ovn::lowlevel::{Ovn, TYPE_LOGICAL_ROUTER_PORT};
use crate::Error;

pub struct LogicalRouterPort {
    ovn: Arc<Ovn>,
    uuid: String,
    name: String,
}

impl LogicalRouterPort {
    pub fn update(&self, networks: &str) -> Result<(), Error> {
        let update_lrp = json!({
            "op": "update",
            "table": TYPE_LOGICAL_ROUTER_PORT,
            "where": [
                ["_uuid", "==", ["uuid", self.uuid()]]
            ],
            "row": {
                "name": &self.name(),
                "mac": "02:00:00:00:00:01",
                "networks": networks,
            }
        });
        self.ovn.transact(&[update_lrp]);
        Ok(())
    }
}

impl OvnCommon for LogicalRouterPort {
    fn uuid(&self) -> String {
        self.uuid.clone()
    }

    fn ovn(&self) -> Arc<Ovn> {
        self.ovn.clone()
    }

    fn ovn_type() -> String {
        TYPE_LOGICAL_ROUTER_PORT.to_owned()
    }

    fn deserialize(ovn: Arc<Ovn>, value: &Value) -> Result<Self, Error> {
        let object = deserialize_object(value)?;

        Ok(LogicalRouterPort {
            ovn,
            uuid: deserialize_uuid(object)?,
            name: deserialize_string(object, "name")?,
        })
    }
}

impl OvnNamed for LogicalRouterPort {
    fn name(&self) -> String {
        self.name.clone()
    }
}
