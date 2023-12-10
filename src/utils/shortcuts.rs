#[macro_export]
macro_rules! create_controller {
    ($resource_type:ident, $reconciler:ident, $error_policy:ident, $context:expr) => {
        Controller::new($resource_type, kube::runtime::watcher::Config::default())
            .run($reconciler, $error_policy, $context)
            .for_each(|res| async move {
                match res {
                    Ok(_o) => { /*println!("reconciled {:?}", o)*/ }
                    Err(e) => error!("reconcile failed: {:?}", e),
                }
            })
            .await
    };
}

#[macro_export]
macro_rules! create_set_status {
    ($resource_type:ident, $resource_status_type:ident, $fn_name:ident) => {
        #[instrument(skip(client))]
        pub async fn $fn_name(resource: &$resource_type, status: $resource_status_type, client: Client) -> Result<(), Error> {
            let api: Api<$resource_type> = Api::namespaced(
                client.clone(),
                &resource.meta().namespace.as_ref().expect("get resource namespace"),
            );
            let status_update = json!({
                "apiVersion": $resource_type::api_version(&()),
                "kind": $resource_type::kind(&()),
                "metadata": {
                    "name": resource.meta().name.as_ref().expect("get resource name"),
                    "resourceVersion": ResourceExt::resource_version(resource),
                },
                "status": status,
            });
            api
                .replace_status(
                    &resource.metadata.name.as_ref().expect("get resource name"),
                    &PostParams::default(),
                    serde_json::to_vec(&status_update).expect("serialize status"),
                )
                .await?;
            Ok(())
        }
    };
    ($resource_type:ident, $resource_status_type:ident) => {
        create_set_status!($resource_type, $resource_status_type, set_status);
    };
}

#[macro_export]
macro_rules! ok_and_requeue {
    ($duration:expr) => {
        Ok(Action::requeue(Duration::from_secs($duration)))
    };
}

#[macro_export]
macro_rules! ok_no_requeue {
    () => {
        Ok(Action::await_change())
    };
}
