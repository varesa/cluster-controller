use serde_json::{Map, Value};

use crate::Error;

pub fn deserialize_object(value: &Value) -> Result<&Map<String, Value>, Error> {
    value.as_object().ok_or(Error::OvnDeserializationFailed)
}

pub fn deserialize_uuid(object: &Map<String, Value>) -> Result<String, Error> {
    object
        .get("_uuid")
        .and_then(|a| a.as_array())
        .and_then(|a| a.get(1))
        .and_then(|u| u.as_str())
        .map(|s| s.to_owned())
        .ok_or(Error::OvnDeserializationFailed)
}

pub fn deserialize_string(object: &Map<String, Value>, field: &str) -> Result<String, Error> {
    object
        .get(field)
        .and_then(|u| u.as_str())
        .map(|s| s.to_owned())
        .ok_or(Error::OvnDeserializationFailed)
}
