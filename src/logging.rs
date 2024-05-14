use crate::errors::Error;
use futures::future::BoxFuture;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::export::trace::{ExportResult, SpanData, SpanExporter};
use opentelemetry_sdk::trace::{Tracer, TracerProvider};
use opentelemetry_sdk::{trace, Resource};
use std::borrow::Cow;
use std::env;
use std::fmt::Debug;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{Layer, Registry};

trait GetAttribute {
    fn get_attribute(&self, name: &str) -> Option<Cow<str>>;
}

impl GetAttribute for SpanData {
    fn get_attribute(&self, name: &str) -> Option<Cow<str>> {
        self.attributes
            .iter()
            .filter_map(|kv: &KeyValue| {
                if kv.key.as_str() == name {
                    Some(kv.value.as_str())
                } else {
                    None
                }
            })
            .next()
    }
}
#[derive(Debug)]
struct Wrapper(opentelemetry_otlp::SpanExporter);

impl SpanExporter for Wrapper {
    fn export(&mut self, batch: Vec<SpanData>) -> BoxFuture<'static, ExportResult> {
        self.0.export(batch)
        /*let new_batch = batch
            .iter()
            .map(|span_data: &SpanData| {
                let mut span_data = span_data.clone();
                if span_data.name == "reconcile resource" {
                    let resource_type = span_data
                        .get_attribute("kind")
                        .unwrap_or(Cow::from("unknown"));
                    span_data.name = Cow::from(format!("reconcile {resource_type}"));
                }
                if span_data.name == "get_by_name" {
                    let resource_type = span_data
                        .get_attribute("kind")
                        .unwrap_or(Cow::from("unknown"));
                    span_data.name = Cow::from(format!("get_by_name {resource_type}"));
                }
                span_data
            })
            .collect();
        self.0.export(new_batch)*/
    }

    fn shutdown(&mut self) {
        self.0.shutdown()
    }

    fn force_flush(&mut self) -> BoxFuture<'static, ExportResult> {
        self.0.force_flush()
    }
}

/// Create a tracing_subscriber layer which exports traces to the given OTLP endpoint.
/// In addition to the tracing_subscriber layer, it also returns a TracerProvider which should be
/// kept in scope in order for the exporter to stay alive. Dropping that will cause the exporter
/// to silenty stop sending traces
fn setup_otlp_layer(
    endpoint: &str,
) -> Result<(TracerProvider, OpenTelemetryLayer<Registry, Tracer>), Error> {
    let otlp_exporter = opentelemetry_otlp::new_exporter()
        .http()
        .with_endpoint(endpoint)
        .build_span_exporter()?;

    let provider = TracerProvider::builder()
        .with_simple_exporter(Wrapper(otlp_exporter))
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

pub fn setup_tracing() -> Result<Option<TracerProvider>, Error> {
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
