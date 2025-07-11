use std::ffi::CStr;
use std::fmt::Formatter;

use crate::metadataservice::protocol::{MetadataRequest, MetadataResponse};
use libc::{c_int, strerror};

fn c_error_name(n: c_int) -> String {
    unsafe {
        let cstr = CStr::from_ptr(strerror(n));
        cstr.to_str().unwrap().to_string()
    }
}

#[derive(Debug)]
pub struct RadosError {
    pub(crate) operation: String,
    pub(crate) code: i32,
}

impl std::error::Error for RadosError {}

impl std::fmt::Display for RadosError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "librados error during {}: {} ({})",
            self.operation,
            c_error_name(self.code),
            self.code
        ))
    }
}

#[derive(Debug)]
pub struct ClusterNotFound {
    pub(crate) name: String,
    pub(crate) inner_error: kube::Error,
}

impl std::error::Error for ClusterNotFound {}

impl std::fmt::Display for ClusterNotFound {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "Cluster {} not found - inner error: {}",
            self.name, self.inner_error,
        ))
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    // stdlib
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    // Tokio
    #[error("Task join error {0}")]
    JoinFailure(#[from] tokio::task::JoinError),

    // Tracing
    #[error("Setting global default subscriber: {0}")]
    TracingSetGlobalDefault(#[from] tracing::dispatcher::SetGlobalDefaultError),
    #[error("OpenTelemetry trace error: {0}")]
    OpenTelemetryTrace(#[from] opentelemetry::trace::TraceError),

    // Kubernetes
    #[error("Kubernetes error {0}")]
    Kube(#[from] kube::Error),
    #[error("Resource watcher error {0}")]
    KubeWatcher(#[from] kube::runtime::watcher::Error),
    #[error("CRD version merge error {0}")]
    CrdMerge(#[from] kube::core::crd::MergeError),
    #[error("Resource {0} has no status")]
    NoStatusSubresource(String),

    // Ceph
    #[error("librados error {0}")]
    Rados(#[from] RadosError),
    #[error("volume locked error")]
    Volumelocked,

    // Libvirt
    #[error("libvirt error {0}")]
    Libvirt(#[from] virt::error::Error),
    #[error("no candidates left to schedule: {0}")]
    ScheduleFailed(String),
    #[error("failed to parse storage location: {0}")]
    StorageLocationParse(String),

    // OVN
    #[error("OVN central nodes not found")]
    OvnCentralNodesNotFound,
    #[error("OVN connection error. Last connection attempt: {0}")]
    OvnConnection(Box<Error>),
    #[error("Object {1} of type {0} not found")]
    OvnNotFound(String, String),
    #[error("Deserialization failed")]
    OvnDeserializationFailed,
    #[error("Conflicting resource: {0}")]
    OvnConflict(String),

    // Misc libs
    #[error("JSON error {0}")]
    Json(#[from] serde_json::Error),
    #[error("Error parsing value: {0}")]
    ParseHumanize(#[from] humanize_rs::ParseError),
    #[error("Error rendering template: {0}")]
    Template(#[from] askama::Error),
    #[error("Error parsing CIDR: {0}")]
    ParseNetwork(#[from] ipnet::AddrParseError),

    // Metadata proxy
    #[error("Failed to send metadata request between threads")]
    RequestSendFailed(#[from] tokio::sync::mpsc::error::SendError<MetadataRequest>),
    #[error("Failed to send metadata response between threads")]
    ResponseSendFailed(#[from] tokio::sync::mpsc::error::SendError<MetadataResponse>),
    #[error("Failed to create network namespace: {0}")]
    NetnsCreateFailed(String),
    #[error("Failed to open network namespace: {0}")]
    NetnsOpenFailed(std::io::Error),
    #[error("Failed to change network namespace: {0}")]
    NetnsChangeFailed(#[from] nix::errno::Errno),
    #[error("Command {0:?} failed: {1}")]
    CommandFailed(Vec<String>, String),
    #[error("Failed to determine instance for metadata: {0}")]
    InstanceMatchFailed(String),
    #[error("Userdata not specified")]
    UserdataNotSpecified,
    #[error("ConfigMap {0} not found")]
    ConfigMapNotFound(String),
    #[error("ConfigMap {0} invalid: {1}")]
    ConfigMapInvalid(String, String),

    // Host network configuration
    #[error("Error mapping VNI: {0}")]
    VniMapping(String),

    // Custom/generic
    #[error("Timed out waiting for operation: {0}")]
    Timeout(String),
    #[error("{0}")]
    ClusterNotFound(#[from] ClusterNotFound),
    #[error("Task ended unexpectedly: {0}")]
    UnexpectedExit(String),
    #[error("Feature not implemented: {0}")]
    NotImplemented(String),
}
