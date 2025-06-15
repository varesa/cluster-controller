use crate::crd::virtualmachine::VirtualMachine;
use crate::errors::Error;
use crate::labels_and_annotations::MIGRATION_REQUEST_ANNOTATION;
use crate::utils::traits::kube::ExtendResource;
use k8s_openapi::api::core::v1::Node;
use kube::{Client, ResourceExt};

pub trait VirtualMachineExt {
    fn migration_requested_from(&self) -> Option<String>;
    async fn request_migration_away_from(
        &mut self,
        source_node: &Node,
        field_manager: &str,
        client: Client,
    ) -> Result<(), crate::Error>;

    async fn clear_migration_request(
        &mut self,
        field_manager: &str,
        client: Client,
    ) -> Result<(), crate::Error>;
}

impl VirtualMachineExt for VirtualMachine {
    fn migration_requested_from(&self) -> Option<String> {
        self.metadata
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.get(MIGRATION_REQUEST_ANNOTATION))
            .cloned()
    }

    async fn request_migration_away_from(
        &mut self,
        source_node: &Node,
        field_manager: &str,
        client: Client,
    ) -> Result<(), crate::Error> {
        self.annotations_mut().insert(
            String::from(MIGRATION_REQUEST_ANNOTATION),
            source_node.name_unchecked(),
        );
        self.commit(client.clone(), field_manager).await?;
        Ok(())
    }

    async fn clear_migration_request(
        &mut self,
        field_manager: &str,
        client: Client,
    ) -> Result<(), Error> {
        self.annotations_mut().remove(MIGRATION_REQUEST_ANNOTATION);
        self.commit(client.clone(), field_manager).await?;
        Ok(())
    }
}
