use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct LogicalSwitch {
    #[serde(rename = "_uuid")]
    pub uuid: String,
    pub name: String,
}

#[derive(Serialize, Deserialize)]
pub struct LogicalSwitchPort {
    #[serde(rename = "_uuid")]
    pub uuid: String,
    pub name: String,
}
