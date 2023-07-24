pub fn get_version_string() -> String {
    format!("{}-{}", env!("GIT_COUNT"), env!("GIT_HASH"))
}

pub fn field_manager(controller: &str) -> String {
    format!("cluster-controller.{controller}")
}
