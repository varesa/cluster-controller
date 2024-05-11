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

pub fn deserialize_uuid_set(
    object: &Map<String, Value>,
    field: &str,
) -> Result<Vec<String>, Error> {
    object
        .get(field)
        .and_then(|a| a.as_array())
        .map(|set| {
            let subtype = set.first().and_then(|v| v.as_str()).unwrap_or("unknown");
            let uuids = match subtype {
                "uuid" => vec![set
                    .get(1)
                    .expect("\"uuid\" should be followed by uuid")
                    .as_str()
                    .expect("value following \"uuid\" should be string")],
                "set" => set
                    .get(1)
                    .expect("\"set\" should be followed by array")
                    .as_array()
                    .expect("value following \"set\" should be an array")
                    .iter()
                    .map(|uuid| {
                        let array = uuid
                            .as_array()
                            .expect("each set element should be an array");
                        assert_eq!(array.first().unwrap(), "uuid");
                        array
                            .get(1)
                            .expect("\"uuid\" should be followed by an uuid")
                            .as_str()
                            .expect("value following \"uuid\" should be a string")
                    })
                    .collect(),
                _ => panic!("Unknown type \"{subtype}\""),
            };
            uuids.iter().map(|s| s.to_string()).collect()
        })
        .ok_or(Error::OvnDeserializationFailed)
}
