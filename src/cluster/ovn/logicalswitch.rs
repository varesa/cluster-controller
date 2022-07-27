use serde_json::Value;

use crate::cluster::ovn::common::{OvnBasicType, OvnCommon, OvnNamed};
use crate::cluster::ovn::lowlevel::TYPE_LOGICAL_SWITCH;
use crate::Error;

pub struct LogicalSwitch {
    uuid: String,
    name: String,
}

macro_rules! try_deserialize {
    ($e:expr) => {
        $e.ok_or_else(|| Error::OvnDeserializationFailed)?
    };
}

impl LogicalSwitch {}

impl OvnCommon for LogicalSwitch {
    fn uuid(&self) -> String {
        self.uuid.clone()
    }

    fn ovn_type() -> String {
        TYPE_LOGICAL_SWITCH.to_owned()
    }

    fn deserialize(value: &Value) -> Result<LogicalSwitch, Error> {
        let object = try_deserialize!(value.as_object());

        Ok(LogicalSwitch {
            uuid: try_deserialize!(object.get("_uuid").and_then(|u| u.as_str())).to_owned(),
            name: try_deserialize!(object.get("name").and_then(|u| u.as_str())).to_owned(),
        })
    }
}

impl OvnNamed for LogicalSwitch {
    fn name(&self) -> String {
        self.name.to_owned()
    }
}

impl OvnBasicType for LogicalSwitch {}
