use serde_json::{Map, Value};

pub struct LogicalSwitch {
    pub uuid: String,
    pub name: String,
}

impl LogicalSwitch {
    pub fn from_json(uuid: &str, parameters: &Map<String, Value>) -> Self {
        LogicalSwitch {
            uuid: uuid.to_string(),
            name: parameters
                .get("name")
                .expect("Switch should have name")
                .as_str()
                .expect("Name should be string")
                .to_string(),
        }
    }
}
