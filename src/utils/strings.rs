use kube::Resource;

pub fn get_version_string() -> String {
    format!("{}-{}", env!("GIT_COUNT"), env!("GIT_HASH"))
}

pub fn name_namespaced<T>(resource: &T) -> String
where
    T: Resource,
{
    format!(
        "{}-{}",
        resource
            .meta()
            .namespace
            .as_ref()
            .expect("get resource namespace"),
        resource.meta().name.as_ref().expect("get resource name")
    )
}
