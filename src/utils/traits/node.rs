use crate::labels_and_annotations::{
    MAINTENANCE_ANNOTATION, NO_SCHEDULE_ANNOTATION, OVN_CENTRAL_IP_ANNOTATION,
    OVN_CENTRAL_MANAGED_LABEL,
};
use k8s_openapi::api::core::v1::Node;
use kube::Resource;

#[derive(Debug, PartialEq)]
pub enum OvnCentralManagement {
    Managed,
    Unmanaged,
    NotPresent,
}

pub trait NodeExt {
    fn in_maintenance_mode(&self) -> bool;
    fn allows_scheduling(&self) -> bool;

    fn ovn_central_status(&self) -> OvnCentralManagement;
    fn ovn_central_annotated_ip(&self) -> Option<String>;
    fn internal_ip(&self) -> Option<String>;
}

impl NodeExt for Node {
    fn in_maintenance_mode(&self) -> bool {
        self.metadata
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.get(MAINTENANCE_ANNOTATION))
            .map(|v| v.to_lowercase())
            == Some(String::from("true"))
    }

    fn allows_scheduling(&self) -> bool {
        self.metadata
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.get(NO_SCHEDULE_ANNOTATION))
            .map(|v| v.to_lowercase())
            != Some(String::from("true"))
    }

    fn ovn_central_status(&self) -> OvnCentralManagement {
        match &self
            .metadata
            .labels
            .as_ref()
            .and_then(|labels| labels.get(OVN_CENTRAL_MANAGED_LABEL))
            .map(|v| v.to_lowercase())
        {
            Some(v) if v == "managed" => OvnCentralManagement::Managed,
            Some(v) if v == "unmanaged" => OvnCentralManagement::Unmanaged,
            Some(_) => OvnCentralManagement::NotPresent,
            None => OvnCentralManagement::NotPresent,
        }
    }

    fn ovn_central_annotated_ip(&self) -> Option<String> {
        self.metadata
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.get(OVN_CENTRAL_IP_ANNOTATION))
            .map(|v| v.clone())
    }

    fn internal_ip(&self) -> Option<String> {
        self.status
            .as_ref()
            .and_then(|status| status.addresses.as_ref())
            .and_then(|addresses| addresses.iter().find(|addr| addr.type_ == "InternalIP"))
            .map(|address| address.address.clone())
    }
}
