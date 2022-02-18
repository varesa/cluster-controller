use serde::{Deserialize, Serialize};
use serde_json::de::Deserializer;
use serde_json::Value;
use std::{io::Write, net::TcpStream};

pub struct JsonRpcConnection {
    stream: TcpStream,
    id: u64,
}

impl JsonRpcConnection {
    pub fn new(host: &str, port: u16) -> Self {
        let stream = TcpStream::connect((host, port)).expect("Failed to connect");
        JsonRpcConnection { stream, id: 0 }
    }

    pub fn request(&mut self, method: &str, params: Option<Value>) -> Response {
        let request = Request {
            id: self.next_id().into(),
            method: method.into(),
            params,
        };
        self.stream
            .write_all(&serde_json::to_vec(&request).unwrap())
            .unwrap();
        let deserializer = Deserializer::from_reader(self.stream.try_clone().unwrap());
        let response: Response = deserializer.into_iter().next().unwrap().unwrap();
        assert!(response.id == request.id);
        return response;
    }

    fn next_id(&mut self) -> u64 {
        let current_id = self.id;
        self.id += 1;
        return current_id;
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: Value,
    pub method: String,
    pub params: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Response {
    pub result: Value,
    pub error: Value,
    pub id: Value,
}
