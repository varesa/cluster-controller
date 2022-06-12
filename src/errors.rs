use libc::{c_int, strerror};
use std::ffi::CStr;
use std::fmt::Formatter;

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
    // Kubernetes
    #[error("Kubernetes error {0}")]
    Kube(#[from] kube::Error),
    #[error("Resource watcher error {0}")]
    KubeWatcher(#[from] kube::runtime::watcher::Error),

    // Ceph
    #[error("librados error {0}")]
    Rados(#[from] RadosError),

    // Libvirt
    #[error("libvirt error {0}")]
    Libvirt(#[from] virt::error::Error),
    #[error("no candidates left to schedule: {0}")]
    ScheduleFailed(String),

    // OVN
    #[error("Object {1} of type {0} not found")]
    OvnNotFound(String, String),

    // Misc libs
    #[error("JSON error {0}")]
    Json(#[from] serde_json::Error),
    #[error("Error parsing value: {0}")]
    ParseHumanize(#[from] humanize_rs::ParseError),
    #[error("Error rendering template: {0}")]
    Template(#[from] askama::Error),
    #[error("Error parsing CIDR: {0}")]
    ParseNetwork(#[from] ipnet::AddrParseError),

    // Custom/generic
    #[error("Timed out waiting for operation: {0}")]
    Timeout(String),
    #[error("{0}")]
    ClusterNotFound(#[from] ClusterNotFound),
}
