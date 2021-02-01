use kube;

use libc::{c_int, strerror};
use std::ffi::CStr;
use serde::export::Formatter;

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

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Kubernetes error {0}")]
    KubeError(#[from] kube::Error),
    #[error("JSON error {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("YAML error {0}")]
    YamlError(#[from] serde_yaml::Error),
    #[error("Resource watcher error {0}")]
    WatcherError(#[from] kube_runtime::watcher::Error),
    #[error("librados error {0}")]
    RadosError(#[from] RadosError),
    #[error("Timed out waiting for operation: {0}")]
    Timeout(String),
    #[error("Error parsing value: {0}")]
    ParseError(#[from] humanize_rs::ParseError),

    /*#[error("data store disconnected")]
    Disconnect(#[from] io::Error),
    #[error("the data for key `{0}` is not available")]
    Redaction(String),
    #[error("invalid header (expected {expected:?}, found {found:?})")]
    InvalidHeader {
        expected: String,
        found: String,
    },
    #[error("unknown data store error")]
    Unknown,*/
}