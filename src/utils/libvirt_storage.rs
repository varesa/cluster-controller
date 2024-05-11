use crate::errors::Error;

#[derive(Debug, Eq, PartialEq)]
pub enum StorageType {
    Ceph,
    Filesystem,
}

/// foo-bar => (Ceph, "foo-bar")
/// ceph:foo-bar => (Ceph, "foo-bar")
/// fs:/foo-bar => (Filesystem, "/foo-bar")
pub fn parse_storage_location(location: &str) -> Result<(StorageType, String), Error> {
    let uri_parts: Vec<&str> = location.split(':').collect();
    if uri_parts.len() == 1 {
        return Ok((StorageType::Ceph, String::from(*uri_parts.first().unwrap())));
    }

    if uri_parts.len() != 2 {
        return Err(Error::StorageLocationParse(String::from(location)));
    }

    let schema = match *uri_parts.first().unwrap() {
        "ceph" => StorageType::Ceph,
        "fs" => StorageType::Filesystem,
        _ => return Err(Error::StorageLocationParse(String::from(location))),
    };

    Ok((schema, String::from(*uri_parts.get(1).unwrap())))
}
