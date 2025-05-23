use crate::errors::Error;
use serde::{Deserialize, Serialize};
use serde_json::de::Deserializer;
use serde_json::Value;
use std::{io::Write, net::TcpStream};
use tracing::{debug, info_span};

#[derive(Debug)]
pub struct JsonRpcConnection {
    stream: TcpStream,
    id: u64,
}

impl JsonRpcConnection {
    pub fn try_new(host: &str, port: u16) -> Result<Self, Error> {
        let stream = TcpStream::connect((host, port))?;
        Ok(JsonRpcConnection { stream, id: 0 })
    }

    pub fn request(&mut self, method: &str, params: &Params) -> Message {
        let span = info_span!(
            "jsonrpc request",
            "method" = method,
            "params" = format!("{params:?}")
        );
        let _s = span.enter();

        span.record("jsonrpc.method", method);
        let request_id: Value = self.next_id().into();
        let request = Message::Request {
            id: request_id.clone(),
            method: method.into(),
            params: params.clone(),
        };

        let request_encoded = serde_json::to_vec(&request).unwrap();
        debug!(
            "jsonrpc: request: {}",
            String::from_utf8(request_encoded.clone()).unwrap()
        );

        self.stream.write_all(&request_encoded).unwrap();
        let deserializer = Deserializer::from_reader(self.stream.try_clone().unwrap());
        let mut iter = deserializer.into_iter();
        while let Some(Ok(message)) = iter.next() {
            debug!("jsonrpc: response: {:?}", &message);
            match message {
                Message::Request { .. } => { /* ignore */ }
                Message::Response {
                    id: ref response_id,
                    ..
                } => {
                    if response_id == &request_id {
                        return message.clone();
                    }
                }
            }
        }
        panic!("no response found");
    }

    fn next_id(&mut self) -> u64 {
        let current_id = self.id;
        self.id += 1;
        current_id
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum Params {
    ByPosition(Vec<serde_json::Value>),
    ByName(serde_json::Map<String, serde_json::Value>),
}

impl Params {
    pub fn from_json(json: serde_json::Value) -> Self {
        match json {
            Value::Array(params) => Params::ByPosition(params),
            Value::Object(params) => Params::ByName(params),
            _ => panic!("Bad JSON value for Params"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum Message {
    Request {
        id: Value,
        method: String,
        params: Params,
    },
    Response {
        id: Value,
        result: Value,
        error: Value,
    },
}
