use kube::Client;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::{Tracer, TracerProvider};
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

fn setup_otlp_layer(endpoint: &str) -> Result<OpenTelemetryLayer<Registry, Tracer>, Error> {
    let otlp_exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(endpoint)
        .build_span_exporter()?;

    let provider = TracerProvider::builder()
        .with_simple_exporter(otlp_exporter)
        .build();

    let tracer = provider.tracer("cluster-controller");

    Ok(tracing_opentelemetry::layer().with_tracer(tracer))
}

fn setup_tracing() -> Result<(), Error> {
    let console_layer = tracing_subscriber::fmt::layer()
        .with_filter(tracing_subscriber::EnvFilter::from_default_env());

    let subscriber = Registry::default();
    let mut layers = Vec::new();

    if let Ok(endpoint) = env::var("OTLP_ENDPOINT") {
        layers.push(setup_otlp_layer(&endpoint)?.boxed());
    }
    layers.push(console_layer.boxed());
    tracing::subscriber::set_global_default(subscriber)?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    println!("Setting up tracing");
    setup_tracing()?;

    info!("Starting up");

    let args: Vec<String> = env::args().collect();

    if args.contains(&String::from("--version")) {
        println!("{}", get_version_string());
        return Ok(());
    }

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
