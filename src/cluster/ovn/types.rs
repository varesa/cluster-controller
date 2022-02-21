use serde::{Deserialize, Serialize};

struct Uuid(Vec<String>);

impl From<Uuid> for String {
    fn from(uuid: Uuid) -> Self {
        uuid.0
            .get(1)
            .expect("Uuid array should have two elements")
            .to_string()
    }
}

#[derive(Serialize, Deserialize)]
pub struct LogicalSwitch {
    pub _uuid: Vec<String>,
    pub name: String,
}

#[derive(Serialize, Deserialize)]
pub struct LogicalSwitchPort {
    pub _uuid: Vec<String>,
    pub name: String,
}

#[derive(Serialize, Deserialize)]
pub struct DhcpOptions {
    pub _uuid: Vec<String>,
    pub cidr: String,
}

impl LogicalSwitch {
    pub fn uuid(&self) -> String {
        assert_eq!(self._uuid.len(), 2);
        self._uuid[1].to_string()
    }
}

impl LogicalSwitchPort {
    #[allow(dead_code)]
    pub fn uuid(&self) -> String {
        assert_eq!(self._uuid.len(), 2);
        self._uuid[1].to_string()
    }
}

impl DhcpOptions {
    pub fn uuid(&self) -> String {
        assert_eq!(self._uuid.len(), 2);
        self._uuid[1].to_string()
    }
}
