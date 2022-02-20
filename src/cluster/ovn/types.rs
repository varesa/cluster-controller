use serde_json::{Map, Value};

pub struct LogicalSwitch {
    pub uuid: String,
    pub name: String,
}

impl LogicalSwitch {
    pub fn from_json(parameters: &Map<String, Value>) -> Self {
        LogicalSwitch {
            uuid: parameters
                .get("_uuid")
                .expect("Switch should have an uuid")
                .as_array()
                .expect("row[*].uuid should be an array")
                .get(1)
                .expect("second element should be the UUID")
                .as_str()
                .expect("UUID should be string")
                .to_owned(),
            name: parameters
                .get("name")
                .expect("Switch should have name")
                .as_str()
                .expect("Name should be string")
                .to_string(),
        }
    }
}

pub struct LogicalSwitchPort {
    pub uuid: String,
    pub name: String,
}

impl LogicalSwitchPort {
    pub fn from_json(parameters: &Map<String, Value>) -> Self {
        LogicalSwitchPort {
            uuid: parameters
                .get("_uuid")
                .expect("Switch should have an uuid")
                .as_array()
                .expect("row[*].uuid should be an array")
                .get(1)
                .expect("second element should be the UUID")
                .as_str()
                .expect("UUID should be string")
                .to_owned(),
            name: parameters
                .get("name")
                .expect("Switch should have name")
                .as_str()
                .expect("Name should be string")
                .to_string(),
        }
    }
}
