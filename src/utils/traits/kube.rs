use crate::errors::Error;
use crate::GROUP_NAME;
use async_trait::async_trait;
use kube::api::PostParams;
use kube::core::object::HasStatus;
use kube::{Api, Client, CustomResourceExt, Resource, ResourceExt};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use tracing::instrument;

pub trait TryStatus {
    type Status;
    fn try_status(&self) -> Result<&Self::Status, Error>;
}

impl<T: HasStatus + ResourceExt + CustomResourceExt> TryStatus for T {
    type Status = T::Status;

    fn try_status(&self) -> Result<&Self::Status, Error> {
        self.status().ok_or(Error::NoStatusSubresource(format!(
            "{}/{} in ns {} has no status",
            T::api_resource().kind,
            self.name_any(),
            self.namespace().unwrap_or(String::from("<no namespace>")),
        )))
    }
}

#[async_trait]
pub trait ExtendResource {
    async fn commit(&mut self, client: Client, field_manager: &str) -> Result<(), Error>;
    fn has_finalizer(&self, finalizer_name: &str) -> bool;
    async fn ensure_finalizer(
        &mut self,
        finalizer_name: &str,
        client: Client,
        field_manager: &str,
    ) -> Result<(), Error>;
    async fn remove_finalizer(
        &mut self,
        finalizer_name: &str,
        client: Client,
        field_manager: &str,
    ) -> Result<(), Error>;
    fn namespace_unchecked(&self) -> String;
    fn name_prefixed_with_namespace(&self) -> String;
}

#[async_trait]
impl<T> ExtendResource for T
where
    for<'a> T: Clone
        + Debug
        + Deserialize<'a>
        + Resource<Scope = k8s_openapi::NamespaceResourceScope>
        + ResourceExt
        + Serialize
        + Sync
        + Send,
    <T as Resource>::DynamicType: Default,
{
    #[instrument(skip(client))]
    async fn commit(&mut self, client: Client, field_manager: &str) -> Result<(), Error> {
        let api: Api<Self> = if let Some(namespace) = self.namespace() {
            Api::namespaced(client, &namespace)
        } else {
            Api::all(client)
        };

        api.replace(
            &self.name_unchecked(),
            &PostParams {
                dry_run: false,
                field_manager: Some(String::from(field_manager)),
            },
            self,
        )
        .await?;
        Ok(())
    }

    fn has_finalizer(&self, finalizer_name: &str) -> bool {
        self.meta()
            .finalizers
            .as_ref()
            .and_then(|finalizers| {
                finalizers
                    .iter()
                    .find(|&finalizer| finalizer == finalizer_name)
            })
            .is_some()
    }

    #[instrument(skip(client))]
    async fn ensure_finalizer(
        &mut self,
        finalizer_name: &str,
        client: Client,
        field_manager: &str,
    ) -> Result<(), Error> {
        let finalizer_name = format!("{GROUP_NAME}/{finalizer_name}");

        if !self.has_finalizer(&finalizer_name) {
            if let Some(finalizers) = self.meta_mut().finalizers.as_mut() {
                finalizers.push(finalizer_name);
            } else {
                self.meta_mut().finalizers = Some(vec![finalizer_name]);
            }
            self.commit(client, field_manager).await?;
        }
        Ok(())
    }

    #[instrument(skip(client))]
    async fn remove_finalizer(
        &mut self,
        finalizer_name: &str,
        client: Client,
        field_manager: &str,
    ) -> Result<(), Error> {
        let finalizer_name = format!("{GROUP_NAME}/{finalizer_name}");

        if self.has_finalizer(&finalizer_name) {
            self.meta_mut()
                .finalizers
                .as_mut()
                .unwrap()
                .retain(|f| f != &finalizer_name);
            self.commit(client, field_manager).await?;
        }
        Ok(())
    }

    fn namespace_unchecked(&self) -> String {
        self.meta()
            .namespace
            .clone()
            .expect(".metadata.namespace missing")
    }

    fn name_prefixed_with_namespace(&self) -> String {
        format!("{}-{}", self.namespace_unchecked(), self.name_unchecked())
    }
}
