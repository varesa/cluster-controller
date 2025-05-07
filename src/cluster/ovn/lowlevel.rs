use super::jsonrpc::{JsonRpcConnection, Message};
use crate::cluster::ovn::jsonrpc::Params;
use crate::errors::Error;
use crate::errors::Error::{OvnCentralNodesNotFound, OvnConnection};
use crate::utils::traits::kube::ApiExt;
use crate::utils::traits::node::NodeExt;
use crate::utils::traits::node::OvnCentralManagement::{Managed, Unmanaged};
use k8s_openapi::api::core::v1::Node;
use kube::{Api, Client};
use serde_json::{json, Map, Value};
use std::sync::Mutex;

pub const TYPE_LOGICAL_SWITCH: &str = "Logical_Switch";
pub const TYPE_LOGICAL_SWITCH_PORT: &str = "Logical_Switch_Port";
pub const TYPE_LOGICAL_ROUTER: &str = "Logical_Router";
pub const TYPE_LOGICAL_ROUTER_PORT: &str = "Logical_Router_Port";
pub const TYPE_LOGICAL_ROUTER_STATIC_ROUTE: &str = "Logical_Router_Static_Route";
pub const TYPE_DHCP_OPTIONS: &str = "DHCP_Options";

#[derive(Debug)]
pub struct Ovn {
    connection: Mutex<JsonRpcConnection>,
}

impl Ovn {
    pub fn try_new(host: &str, port: u16) -> Result<Self, Error> {
        Ok(Ovn {
            connection: Mutex::new(JsonRpcConnection::try_new(host, port)?),
        })
    }

    pub async fn try_from_annotations(client: Client) -> Result<Self, Error> {
        let node_api: Api<Node> = Api::all(client.clone());
        let nodes = node_api.list_default().await?;
        let ovn_central_nodes = nodes.iter().filter(|node| {
            let ovn_central_status = node.ovn_central_status();
            ovn_central_status == Managed || ovn_central_status == Unmanaged
        });

        let mut error = None;
        for node in ovn_central_nodes {
            if let Some(addresses) = node
                .status
                .as_ref()
                .and_then(|status| status.addresses.as_ref())
            {
                for address in addresses {
                    if address.type_ == "InternalIP" {
                        let attempt = Self::try_new(address.address.as_str(), 6641);
                        if attempt.is_ok() {
                            return attempt;
                        } else {
                            error = Some(attempt.err().unwrap());
                            tracing::error!(
                                "Failed to connect to OVN-Central on {:?}: {:?}",
                                address.address,
                                error
                            );
                        }
                    }
                }
            }
        }
        if let Some(error) = error {
            Err(OvnConnection(Box::new(error)))
        } else {
            Err(OvnCentralNodesNotFound)
        }
    }

    pub fn transact(&self, operations: &[Value]) -> Vec<Value> {
        tracing::info!("transact");
        let mut params = vec![Value::String("OVN_Northbound".to_string())];
        params.append(&mut operations.to_owned());
        let response = self
            .connection
            .lock()
            .expect("Lock poisoned")
            .request("transact", &Params::from_json(json!(params)));
        match response {
            Message::Response { error, result, .. } => {
                assert!(error.is_null());
                let results = result.as_array().expect("result not an array").to_owned();
                for result in results.iter() {
                    let error = result
                        .as_object()
                        .expect("result should be an object")
                        .get("error");
                    assert!(error.is_none() || error.unwrap().is_null());
                }
                results
            }
            _ => panic!("Didn't get response"),
        }
    }

    pub fn list_objects(&self, object_type: &str) -> Vec<Value> {
        let columns = match object_type {
            TYPE_DHCP_OPTIONS => json!(["_uuid", "cidr"]),
            TYPE_LOGICAL_ROUTER => json!(["_uuid", "name", "static_routes"]),
            TYPE_LOGICAL_ROUTER_STATIC_ROUTE => json!(["_uuid", "ip_prefix", "nexthop"]),
            TYPE_LOGICAL_SWITCH => json!(["_uuid", "name", "ports"]),
            TYPE_LOGICAL_SWITCH_PORT => json!(["_uuid", "name", "addresses", "dynamic_addresses"]),
            _ => json!(["_uuid", "name"]),
        };
        let select = json!({
            "op": "select",
            "table": object_type,
            "where": [],
            "columns": columns
        });
        self.transact(&[select])[0]
            .as_object()
            .expect("Transaction result not an object")
            .get("rows")
            .expect("Transaction didn't return rows")
            .as_array()
            .expect("Rows is not an array")
            .to_owned()
    }

    pub fn insert(&self, object_type: &str, params: Map<String, Value>) {
        tracing::info!("insert");
        let operation = json!({
            "op": "insert",
            "table": object_type,
            "row": params
        });
        self.transact(&[operation]);
    }

    pub fn delete_by_uuid(&self, object_type: &str, uuid: &str) {
        let operation = json!({
            "op": "delete",
            "table": object_type,
            "where": [
                ["_uuid", "==", ["uuid", uuid]]
            ]
        });
        self.transact(&[operation]);
    }
}
