use std::sync::Arc;

use serde_json::{json, Value};

use crate::cluster::ovn::common::{OvnCommon, OvnNamed, OvnNamedGetters};
use crate::cluster::ovn::deserialization::{
    deserialize_object, deserialize_string, deserialize_uuid,
};
use crate::cluster::ovn::logicalrouter::LogicalRouter;
use crate::cluster::ovn::lowlevel::{Ovn, TYPE_LOGICAL_ROUTER, TYPE_LOGICAL_ROUTER_PORT};
use crate::Error;

pub struct LogicalRouterPortBuilder<'a> {
    pub ovn: Arc<Ovn>,
    pub lr: &'a LogicalRouter,
}

impl LogicalRouterPortBuilder<'_> {
    pub fn create(self, lrp_name: &str, networks: &str) -> Result<LogicalRouterPort, Error> {
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
                ["_uuid", "==", ["uuid", self.lr.uuid()]]
            ],
            "mutations": [
                ["ports", "insert", ["set", [["named-uuid", "new_lrp"]]]]
            ]
        });
        self.ovn.transact(&[add_lrp, add_lrp_to_lr]);
        LogicalRouterPort::get_by_name(self.ovn.clone(), lrp_name)
    }

    pub fn create_if_missing(
        self,
        lrp_name: &str,
        networks: &str,
    ) -> Result<LogicalRouterPort, Error> {
        match LogicalRouterPort::get_by_name(self.ovn.clone(), lrp_name) {
            Ok(lr) => Ok(lr),
            Err(Error::OvnNotFound(_, _)) => {
                println!(
                    "ovn: {} {} doesn't exist, creating",
                    LogicalRouterPort::ovn_type(),
                    lrp_name
                );
                self.create(lrp_name, networks)
            }
            Err(e) => Err(e),
        }
    }
}

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
