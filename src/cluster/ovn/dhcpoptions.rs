use std::net::IpAddr;
use std::sync::Arc;

use ipnet::IpNet;
use serde_json::{json, Value};

use crate::cluster::ovn::common::{OvnCommon, OvnGetters};
use crate::cluster::ovn::lowlevel::{Ovn, TYPE_DHCP_OPTIONS};
use crate::crd::ovn::DhcpOptions as DhcpOptionsCrd;
use crate::{try_deserialize, Error};

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
            [String::from("server_id"), hosts[1].to_string()],
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
                    $destination.push([String::from(stringify!($name)), value.to_string()]);
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
        let object = try_deserialize!(value.as_object());

        Ok(DhcpOptions {
            ovn,
            uuid: try_deserialize!(object
                .get("_uuid")
                .and_then(|a| a.as_array())
                .and_then(|a| a.get(1))
                .and_then(|u| u.as_str()))
            .to_owned(),
            cidr: try_deserialize!(object.get("cidr").and_then(|u| u.as_str())).to_owned(),
        })
    }
}
