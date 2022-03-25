use serde::{Deserialize, Serialize};

macro_rules! create_type {
    ($type_name:ident) => {
        #[derive(Serialize, Deserialize)]
        pub struct $type_name {
            pub _uuid: Vec<String>,
            pub name: String,
        }

        impl $type_name {
            #[allow(unused)]
            pub fn uuid(&self) -> String {
                assert_eq!(self._uuid.len(), 2);
                self._uuid[1].to_string()
            }
        }
    };
}


create_type!(LogicalSwitch);
create_type!(LogicalSwitchPort);
create_type!(LogicalRouter);
create_type!(LogicalRouterPort);


#[derive(Serialize, Deserialize)]
pub struct DhcpOptions {
    pub _uuid: Vec<String>,
    pub cidr: String,
}

impl DhcpOptions {
    pub fn uuid(&self) -> String {
        assert_eq!(self._uuid.len(), 2);
        self._uuid[1].to_string()
    }
}

