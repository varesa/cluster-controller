use crate::cluster::MAINTENANCE_ANNOTATION;
use k8s_openapi::api::core::v1::Node;

pub trait NodeExt {
    fn in_maintenance_mode(&self) -> bool;
}

impl NodeExt for Node {
    fn in_maintenance_mode(&self) -> bool {
        if let Some(annotations) = self.metadata.annotations.as_ref()
            && let Some(value) = annotations.get(MAINTENANCE_ANNOTATION)
            && value.to_lowercase() == "true"
        {
            true
        } else {
            false
        }
    }
}
