use crate::cluster::{MAINTENANCE_ANNOTATION, NO_SCHEDULE_ANNOTATION};
use k8s_openapi::api::core::v1::Node;

pub trait NodeExt {
    fn in_maintenance_mode(&self) -> bool;
    fn allows_scheduling(&self) -> bool;
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
}
