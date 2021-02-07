use virt::connect::{
    Connect,
    VIR_CONNECT_LIST_DOMAINS_ACTIVE,
    VIR_CONNECT_LIST_DOMAINS_INACTIVE,
};
use virt::domain::Domain;

use crate::errors::Error;

pub struct Libvirt {
    connection: Connect,
}

/// virt::connect::Connect does not implement Send due to the raw pointer
/// to the virConnect instance. However according to the library FAQ the
/// library is thread-safe:
/// > Yes, libvirt is thread safe as of version 0.6.0. This means that
/// > multiple threads can act on a single virConnect instance without issue.
unsafe impl Send for Libvirt {}
unsafe impl Sync for Libvirt {}

impl Drop for Libvirt {
    fn drop(&mut self) {
        self.connection.close();
    }
}

impl Libvirt {
    pub fn new(uri: &str) -> Result<Self, Error> {
        let connection = Connect::open(uri);
        match connection {
            Ok(connection) => Ok(Self { connection }),
            Err(err) => Err(err.into()),
        }
    }

    pub fn get_all_domains(&self) -> Result<Vec<Domain>, Error> {
        let flags = 0; // 0 => all domains
        match self.connection.list_all_domains(flags) {
            Ok(domains) => Ok(domains),
            Err(err) => Err(err.into()),
        }
    }

    pub fn get_domain(&self, name: &str) -> Result<Domain, Error> {
        let domain = self.get_all_domains()?
            .into_iter()
            .find(|domain| domain.get_name().expect("get domain name") == name);
        match domain {
            Some(domain) => Ok(domain),
            None => Err(Error::LibvirtDomainNotFound(String::from(name))),
        }
    }
}