use kube;
use std::convert::From;

macro_rules! generate_error {
    { $name:ident: [$($type:ident: $content:ty,)+]} => {
        #[derive(Debug)]
        pub enum $name {
            $(
                $type($content),
            )+
        }

        $(
            impl From<$content> for $name {
                fn from(err: $content) -> Self {
                    $name::$type(err)
                }
            }
        )+
    }
}

generate_error! {
    Error: [
        MultipleErrors: Vec<Error>,
        JoinError: tokio::task::JoinError,
        KubeError: kube::Error,
        WatcherError: kube_runtime::watcher::Error,
        SerdeJsonError: serde_json::Error,
        //PodFailure: PodFailure,
        //Timeout: Timeout,
    ]
}
