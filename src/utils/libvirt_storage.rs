use crate::errors::Error;

#[derive(Debug, Eq, PartialEq)]
pub enum StorageType {
    Ceph,
    Filesystem,
}

/// foo-bar => (Ceph, "foo-bar")
/// ceph:foo-bar => (Ceph, "foo-bar")
/// node1:/foo-bar => (Filesystem, "/foo-bar")
pub fn parse_storage_location(location: &str) -> Result<(StorageType, String, String), Error> {
    let uri_parts: Vec<&str> = location.split(':').collect();
    if uri_parts.len() == 1 || uri_parts.first().unwrap() == &"ceph" {
        Ok((
            StorageType::Ceph,
            String::from(""),
            String::from(*uri_parts.first().unwrap()),
        ))
    } else if uri_parts.len() == 2 && uri_parts.first().unwrap() != &"ceph" {
        Ok((
            StorageType::Filesystem,
            String::from(*uri_parts.first().unwrap()),
            String::from(*uri_parts.get(1).unwrap()),
        ))
    } else {
        Err(Error::StorageLocationParse(String::from(location)))
    }
}
