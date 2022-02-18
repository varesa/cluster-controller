use virt::connect::Connect;

use crate::errors::Error;

pub struct Libvirt {
    pub connection: Connect,
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
        self.connection.close().expect("close libvirt connection");
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
}
