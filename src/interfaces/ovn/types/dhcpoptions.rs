use std::net::IpAddr;
use std::sync::Arc;

use ipnet::IpNet;
use serde_json::{Value, json};

use crate::Error;
use crate::crd::network::DhcpOptions as DhcpOptionsCrd;
use crate::interfaces::ovn::common::{OvnCommon, OvnGetters};
use crate::interfaces::ovn::deserialization::{
    deserialize_object, deserialize_string, deserialize_uuid,
};
use crate::interfaces::ovn::lowlevel::{Ovn, TYPE_DHCP_OPTIONS};

pub struct DhcpOptions {
    ovn: Arc<Ovn>,
    uuid: String,
    cidr: String,
}

impl DhcpOptions {
    pub fn create(ovn: Arc<Ovn>, cidr: &str) -> Result<DhcpOptions, Error> {
        let create_options = json!({
            "op": "insert",
            "table": TYPE_DHCP_OPTIONS,
            "row": {"cidr": cidr},
            "uuid-name": "new_dhcp_options"
        });
        ovn.transact(&[create_options]);
        DhcpOptions::get_by_cidr(ovn, cidr)
    }

    pub fn get_by_cidr(ovn: Arc<Ovn>, cidr: &str) -> Result<DhcpOptions, Error> {
        Self::list(ovn)?
            .into_iter()
            .find(|o| o.cidr == cidr)
            .ok_or_else(|| Error::OvnNotFound(Self::ovn_type(), cidr.to_string()))
    }

    pub fn set_options(&mut self, dhcp_options: &DhcpOptionsCrd) -> Result<(), Error> {
        let net: IpNet = self.cidr.parse()?;
        let hosts: Vec<IpAddr> = net.hosts().collect();
        let mut options = vec![
            [String::from("server_id"), hosts[0].to_string()],
            [
                String::from("server_mac"),
                String::from("c0:ff:ee:00:00:01"),
            ],
        ];

        // Copy values from CRD to above vector
        macro_rules! push_dhcp_opts {
            ($source:ident, $destination:ident, [$($name:ident),+]) => {
                $(
                if let Some(value) = $source.$name.clone() {
                    let value = if stringify!($name) == "domain_name" {
                        format!("\"{}\"", value)
                    } else {
                        value.to_string()
                    };
                    $destination.push([String::from(stringify!($name)), value]);
                }
                )+
            }
        }
        push_dhcp_opts!(
            dhcp_options,
            options,
            [lease_time, dns_server, domain_name, router]
        );

        let set_options = json!({
            "op": "update",
            "table": TYPE_DHCP_OPTIONS,
            "where": [["_uuid", "==", ["uuid", self.uuid()]]],
            "row": {"options": ["map", options]}
        });
        self.ovn.transact(&[set_options]);
        Ok(())
    }
}

impl OvnCommon for DhcpOptions {
    fn uuid(&self) -> String {
        self.uuid.clone()
    }

    fn ovn(&self) -> Arc<Ovn> {
        self.ovn.clone()
    }

    fn ovn_type() -> String {
        TYPE_DHCP_OPTIONS.to_owned()
    }

    fn deserialize(ovn: Arc<Ovn>, value: &Value) -> Result<Self, Error> {
        let object = deserialize_object(value)?;

        Ok(DhcpOptions {
            ovn,
            uuid: deserialize_uuid(object)?,
            cidr: deserialize_string(object, "cidr")?,
        })
    }
}
