mod network;
mod ovn_vm;
mod router;

use crate::integration_tests::harness::TestResult;
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    time::Duration,
};
use tokio::{
    task::JoinHandle,
    time::{sleep, timeout},
};

struct AbortOnDrop<T>(JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn wait_for<F, Fut>(description: &str, mut condition: F) -> TestResult
where
    F: FnMut() -> Fut,
    Fut: Future<Output = TestResult<bool>>,
{
    timeout(Duration::from_secs(30), async {
        loop {
            if condition().await? {
                return Ok::<(), Box<dyn std::error::Error + Send + Sync>>(());
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .map_err(|_| format!("timed out waiting for {description}"))??;
    Ok(())
}

fn ovsdb_column<'a>(row: &'a Map<String, Value>, column: &str) -> TestResult<&'a Value> {
    row.get(column)
        .ok_or_else(|| format!("OVSDB row has no {column} column: {row:?}").into())
}

fn ovsdb_set(value: &Value) -> TestResult<Vec<&Value>> {
    let Some(encoded) = value.as_array() else {
        return Ok(vec![value]);
    };
    if encoded.first().and_then(Value::as_str) != Some("set") {
        return Ok(vec![value]);
    }
    encoded
        .get(1)
        .and_then(Value::as_array)
        .map(|values| values.iter().collect())
        .ok_or_else(|| format!("invalid OVSDB set: {value}").into())
}

fn ovsdb_string_map(value: &Value) -> TestResult<BTreeMap<String, String>> {
    let entries = value
        .as_array()
        .filter(|encoded| encoded.first().and_then(Value::as_str) == Some("map"))
        .and_then(|encoded| encoded.get(1))
        .and_then(Value::as_array)
        .ok_or_else(|| format!("invalid OVSDB string map: {value}"))?;
    entries
        .iter()
        .map(|entry| {
            let fields = entry
                .as_array()
                .filter(|fields| fields.len() == 2)
                .ok_or_else(|| format!("invalid OVSDB map entry: {entry}"))?;
            let key = fields[0]
                .as_str()
                .ok_or_else(|| format!("OVSDB map key is not a string: {entry}"))?;
            let value = fields[1]
                .as_str()
                .ok_or_else(|| format!("OVSDB map value is not a string: {entry}"))?;
            Ok((key.to_owned(), value.to_owned()))
        })
        .collect()
}

fn ovsdb_string_set(value: &Value) -> TestResult<BTreeSet<String>> {
    ovsdb_set(value)?
        .into_iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("OVSDB set member is not a string: {item}").into())
        })
        .collect()
}

fn ovsdb_uuid(value: &Value) -> TestResult<&str> {
    value
        .as_array()
        .filter(|encoded| encoded.first().and_then(Value::as_str) == Some("uuid"))
        .and_then(|encoded| encoded.get(1))
        .and_then(Value::as_str)
        .ok_or_else(|| format!("invalid OVSDB UUID: {value}").into())
}

fn ovsdb_uuid_set(value: &Value) -> TestResult<BTreeSet<String>> {
    ovsdb_set(value)?
        .into_iter()
        .map(|item| ovsdb_uuid(item).map(str::to_owned))
        .collect()
}
