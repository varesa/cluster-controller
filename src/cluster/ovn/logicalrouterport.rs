use std::sync::Arc;

use serde_json::{json, Value};

use crate::cluster::ovn::common::{OvnCommon, OvnNamed};
use crate::cluster::ovn::lowlevel::{Ovn, TYPE_LOGICAL_ROUTER_PORT};
use crate::{try_deserialize, Error};

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
        let object = try_deserialize!(value.as_object());

        Ok(LogicalRouterPort {
            ovn,
            uuid: try_deserialize!(object
                .get("_uuid")
                .and_then(|a| a.as_array())
                .and_then(|a| a.get(1))
                .and_then(|u| u.as_str()))
            .to_owned(),
            name: try_deserialize!(object.get("name").and_then(|u| u.as_str())).to_owned(),
        })
    }
}

impl OvnNamed for LogicalRouterPort {
    fn name(&self) -> String {
        self.name.clone()
    }
}
