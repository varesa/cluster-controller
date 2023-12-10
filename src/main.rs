use kube::Client;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::{Tracer, TracerProvider};
use opentelemetry_sdk::{trace, Resource};
use std::env;
use tracing::info;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::prelude::*;
use tracing_subscriber::Registry;

use crate::errors::Error;
use crate::utils::strings::get_version_string;

mod cluster;
mod errors;
mod host;
#[macro_use]
mod utils;
mod crd;
mod metadataservice;
mod shared;

const NAMESPACE: &str = "virt-controller";
const GROUP_NAME: &str = "cluster-virt.acl.fi";
const KEYRING_SECRET: &str = "ceph-client.libvirt";

/// Create a tracing_subscriber layer which exports traces to the given OTLP endpoint.
/// In addition to the tracing_subscriber layer, it also returns a TracerProvider which should be
/// kept in scope in order for the exporter to stay alive. Dropping that will cause the exporter
/// to silenty stop sending traces
fn setup_otlp_layer(
    endpoint: &str,
) -> Result<(TracerProvider, OpenTelemetryLayer<Registry, Tracer>), Error> {
    let otlp_exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(endpoint)
        .build_span_exporter()?;

    let provider = TracerProvider::builder()
        .with_simple_exporter(otlp_exporter)
        .with_config(
            trace::config().with_resource(Resource::new(vec![KeyValue::new(
                "service.name",
                "cluster-controller",
            )])),
        )
        .build();

    let tracer = provider.tracer("cluster-controller");
    let layer = tracing_opentelemetry::layer().with_tracer(tracer);

    // We must return provider to prevent it from being dropped
    Ok((provider, layer))
}

fn setup_tracing() -> Result<Option<TracerProvider>, Error> {
    let console_layer = tracing_subscriber::fmt::layer()
        .compact()
        .with_filter(tracing_subscriber::EnvFilter::from_default_env());

    let subscriber = Registry::default();
    let mut layers = Vec::new();

    let mut provider = None;
    if let Ok(endpoint) = env::var("OTLP_ENDPOINT") {
        println!("Adding OTLP export");
        let (tracer_provider, exporter_layer) = setup_otlp_layer(&endpoint)?;
        layers.push(
            exporter_layer
                .with_filter(tracing_subscriber::EnvFilter::from_default_env())
                .boxed(),
        );
        provider = Some(tracer_provider);
    }
    layers.push(console_layer.boxed());
    tracing::subscriber::set_global_default(subscriber.with(layers))?;

    Ok(provider)
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let args: Vec<String> = env::args().collect();

    if args.contains(&String::from("--version")) {
        // This is used by packaging scripts, ensure no other output gets printed
        println!("{}", get_version_string());
        return Ok(());
    }

    println!("Setting up tracing");
    let _provider = setup_tracing()?;

    info!("Starting up");

    let client = Client::try_default().await?;

    if args.contains(&String::from("--host")) {
        info!("Starting host-mode");
        host::libvirt::run(client).await?;
    } else if args.contains(&String::from("--metadata-service")) {
        info!("Staring metadata service mode");
        metadataservice::run(args, client).await?;
    } else {
        info!("Starting cluster-mode");
        cluster::run(client, NAMESPACE).await?;
    }

    Ok(())
}
