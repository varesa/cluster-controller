use serde_json::{json, Map, Value};
use std::collections::HashMap;

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

#[cfg(test)]
#[test]
fn test_deserialize_uuid() {
    let data = json!({"_uuid": ["uuid", "1-2-3"]});
    assert_eq!(
        deserialize_uuid(deserialize_object(&data).unwrap()).unwrap(),
        "1-2-3".to_string()
    );
}

pub fn deserialize_string(object: &Map<String, Value>, field: &str) -> Result<String, Error> {
    object
        .get(field)
        .and_then(|u| u.as_str())
        .map(|s| s.to_owned())
        .ok_or(Error::OvnDeserializationFailed)
}

#[cfg(test)]
#[test]
fn test_deserialize_string() {
    let data = json!({"property": "value"});
    assert_eq!(
        deserialize_string(deserialize_object(&data).unwrap(), "property").unwrap(),
        "value".to_string()
    );
}

pub fn deserialize_map(
    object: &Map<String, Value>,
    field: &str,
) -> Result<HashMap<String, String>, Error> {
    object
        .get(field)
        .and_then(|array_value| array_value.as_array())
        .map(|map_as_array| {
            // ["map", [ [k,v], [k,v] ]]
            assert_eq!(map_as_array.len(), 2);
            let mut iter = map_as_array.iter();
            assert_eq!(iter.next(), Some(&Value::String("map".into())));
            iter.next().unwrap()
        })
        .and_then(|k_v_array_val| k_v_array_val.as_array())
        .map(|map_entries| {
            // [ [k,v], [k,v] ]
            map_entries.iter().map(|entry| {
                // [k,v]
                let k_v_array = entry
                    .as_array()
                    .expect("map entries should have a key and a value");
                assert_eq!(k_v_array.len(), 2);

                let k = k_v_array[0].as_str().unwrap().to_owned();
                let v = k_v_array[1].as_str().unwrap().to_owned();

                (k, v)
            })
        })
        .map(|x| x.collect())
        .ok_or(Error::OvnDeserializationFailed)
}

#[cfg(test)]
#[test]
fn test_deserialize_map() {
    let data = json!({"config": ["map", [["k1", "v1"], ["k2", "v2"]]]});
    assert_eq!(
        deserialize_map(deserialize_object(&data).unwrap(), "config").unwrap(),
        HashMap::from([("k1".into(), "v1".into()), ("k2".into(), "v2".into())])
    );
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
                "uuid" => vec![
                    set.get(1)
                        .expect("\"uuid\" should be followed by uuid")
                        .as_str()
                        .expect("value following \"uuid\" should be string"),
                ],
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

#[cfg(test)]
#[test]
fn test_deserialize_uuid_set() {
    let data1 = json!({"uuids": ["uuid", "1-2-3"]});
    let data2 = json!({"uuids": ["set", [["uuid", "1-2-3"]]]});
    assert_eq!(
        deserialize_uuid_set(deserialize_object(&data1).unwrap(), "uuids").unwrap(),
        Vec::from(["1-2-3"])
    );
    assert_eq!(
        deserialize_uuid_set(deserialize_object(&data2).unwrap(), "uuids").unwrap(),
        Vec::from(["1-2-3"])
    );
}
