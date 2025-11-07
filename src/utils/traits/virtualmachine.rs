use crate::crd::network::Network;
use crate::crd::virtualmachine::VirtualMachine;
use crate::errors::Error;
use crate::labels_and_annotations::MIGRATION_REQUEST_ANNOTATION;
use crate::utils::traits::kube::ExtendResource;
use crate::utils::traits::node::NetworkModel;
use k8s_openapi::api::core::v1::Node;
use kube::{Api, Client, ResourceExt};

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

    /// Determine the VM's network model usage by inspecting its interfaces and, when a managed
    /// Network name is referenced, resolving the corresponding Network object in Kubernetes within
    /// the same loop.
    ///
    /// Rules:
    /// - For NICs with `name`:
    ///   - If the Network has `spec.network_id` set → SingleBridge.
    ///   - If the Network has `spec.bridge` but no `spec.network_id` → Legacy.
    ///   - If the Network is missing, cannot be fetched, or ambiguous → bias to SingleBridge.
    /// - For NICs with `bridge`:
    ///   - If no VLAN/network ID is provided, assume Legacy.
    ///   - If VLAN tagging is present (untagged or tagged list), treat as SingleBridge.
    /// - If both models are present across interfaces, default to SingleBridge.
    /// - If no networks defined, default to SingleBridge.
    async fn network_model_used(&self, client: Client) -> NetworkModel;
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

    async fn network_model_used(&self, client: Client) -> NetworkModel {
        let mut saw_legacy = false;
        let mut saw_single = false;

        // VM is a namespaced resource; namespace is always present
        let ns = self.namespace_unchecked();
        let networks_api: Api<Network> = Api::namespaced(client, &ns);

        for nic in &self.spec.networks {
            // If a managed Network is referenced by name, resolve it inline
            if let Some(name) = &nic.name {
                if let Ok(net) = networks_api.get(name).await {
                    let has_id = net.spec.network_id.is_some();
                    let has_bridge = net.spec.bridge.is_some();
                    if has_bridge && has_id {
                        saw_single = true;
                    } else if has_bridge {
                        // Managed network on a specific bridge without id → legacy style
                        saw_legacy = true;
                    }
                }
            }

            if nic.bridge.is_some() {
                // Bridge path
                let vlan_present = nic.untagged_vlan.is_some()
                    || nic
                        .tagged_vlans
                        .as_ref()
                        .map(|v| !v.is_empty())
                        .unwrap_or(false);
                if vlan_present {
                    // Bridge with VLANs implies new model awareness
                    saw_single = true;
                } else {
                    // Bridge without any network id → legacy
                    saw_legacy = true;
                }
            }
        }

        if saw_single {
            NetworkModel::SingleBridge
        } else if saw_legacy {
            NetworkModel::Legacy
        } else {
            NetworkModel::OvnOnly
        }
    }
}
