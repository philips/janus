//! Collection and exporting of application-level metrics for Janus.

#[cfg(any(not(feature = "prometheus"), not(feature = "otlp")))]
use anyhow::anyhow;
use opentelemetry::{
    metrics::{Counter, Meter, Unit},
    KeyValue,
};
use serde::{Deserialize, Serialize};
use std::net::AddrParseError;

#[cfg(feature = "prometheus")]
use {
    anyhow::Context,
    opentelemetry::global::set_meter_provider,
    prometheus::Registry,
    std::{
        net::{IpAddr, Ipv4Addr},
        str::FromStr,
    },
    tokio::{sync::oneshot, task::JoinHandle},
    trillium::{Info, Init},
};

#[cfg(feature = "otlp")]
use {
    opentelemetry_otlp::WithExportConfig,
    opentelemetry_sdk::{
        metrics::{
            reader::{DefaultAggregationSelector, DefaultTemporalitySelector},
            PeriodicReader,
        },
        runtime::Tokio,
    },
};

#[cfg(any(feature = "otlp", feature = "prometheus"))]
use {
    crate::git_revision,
    janus_aggregator_core::datastore::TRANSACTION_RETRIES_METER_NAME,
    opentelemetry::metrics::MetricsError,
    opentelemetry_sdk::{
        metrics::{
            new_view, Aggregation, Instrument, InstrumentKind, SdkMeterProvider, Stream, View,
        },
        Resource,
    },
};

#[cfg(all(test, feature = "prometheus"))]
mod tests;

/// Errors from initializing metrics provider, registry, and exporter.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("bad IP address: {0}")]
    IpAddress(#[from] AddrParseError),
    #[error(transparent)]
    OpenTelemetry(#[from] opentelemetry::metrics::MetricsError),
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

/// Configuration for collection/exporting of application-level metrics.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MetricsConfiguration {
    /// Configuration for OpenTelemetry metrics, with a choice of exporters.
    #[serde(default, with = "serde_yaml::with::singleton_map")]
    pub exporter: Option<MetricsExporterConfiguration>,
}

/// Selection of an exporter for OpenTelemetry metrics.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MetricsExporterConfiguration {
    Prometheus {
        host: Option<String>,
        port: Option<u16>,
    },
    Otlp(OtlpExporterConfiguration),
}

/// Configuration options specific to the OpenTelemetry OTLP metrics exporter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OtlpExporterConfiguration {
    /// gRPC endpoint for OTLP exporter.
    pub endpoint: String,
}

/// Choice of OpenTelemetry metrics exporter implementation.
pub enum MetricsExporterHandle {
    #[cfg(feature = "prometheus")]
    Prometheus {
        handle: JoinHandle<()>,
        port: u16,
    },
    #[cfg(feature = "otlp")]
    Otlp(SdkMeterProvider),
    Noop,
}

#[cfg(any(feature = "prometheus", feature = "otlp"))]
struct CustomView {
    uint_histogram_view: Box<dyn View>,
    bytes_histogram_view: Box<dyn View>,
    default_histogram_view: Box<dyn View>,
}

#[cfg(any(feature = "prometheus", feature = "otlp"))]
impl CustomView {
    /// These boundaries are intended to be used with measurements having the unit of "bytes".
    const BYTES_HISTOGRAM_BOUNDARIES: &'static [f64] = &[
        1024.0, 4096.0, 16384.0, 65536.0, 262144.0, 1048576.0, 4194304.0, 8388608.0, 16777216.0,
        33554432.0, 67108864.0,
    ];

    /// These boundaries are for measurements of unsigned integers, such as the number of retries
    /// that an operation took.
    const UINT_HISTOGRAM_BOUNDARIES: &'static [f64] = &[
        1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0, 256.0, 512.0, 1024.0, 2048.0, 4096.0, 8192.0,
        16384.0,
    ];

    /// These boundaries are intended to be able to capture the length of short-lived operations
    /// (e.g HTTP requests) as well as longer-running operations.
    const DEFAULT_HISTOGRAM_BOUNDARIES: &'static [f64] = &[
        0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 90.0, 300.0,
    ];

    pub fn new() -> Result<Self, MetricsError> {
        Ok(Self {
            uint_histogram_view: new_view(
                Instrument::new().name("*"),
                Stream::new().aggregation(Aggregation::ExplicitBucketHistogram {
                    boundaries: Vec::from(Self::UINT_HISTOGRAM_BOUNDARIES),
                    record_min_max: true,
                }),
            )?,
            bytes_histogram_view: new_view(
                Instrument::new().name("*"),
                Stream::new().aggregation(Aggregation::ExplicitBucketHistogram {
                    boundaries: Vec::from(Self::BYTES_HISTOGRAM_BOUNDARIES),
                    record_min_max: true,
                }),
            )?,
            default_histogram_view: new_view(
                Instrument::new().name("*"),
                Stream::new().aggregation(Aggregation::ExplicitBucketHistogram {
                    boundaries: Vec::from(Self::DEFAULT_HISTOGRAM_BOUNDARIES),
                    record_min_max: true,
                }),
            )?,
        })
    }
}

#[cfg(any(feature = "prometheus", feature = "otlp"))]
impl View for CustomView {
    fn match_inst(&self, inst: &Instrument) -> Option<Stream> {
        match (inst.kind, inst.name.as_ref()) {
            (
                Some(InstrumentKind::Histogram),
                "http.server.request.body.size" | "http.server.response.body.size",
            ) => self.bytes_histogram_view.match_inst(inst),
            (Some(InstrumentKind::Histogram), TRANSACTION_RETRIES_METER_NAME) => {
                self.uint_histogram_view.match_inst(inst)
            }
            (Some(InstrumentKind::Histogram), _) => self.default_histogram_view.match_inst(inst),
            _ => None,
        }
    }
}

#[cfg(feature = "prometheus")]
fn build_opentelemetry_prometheus_meter_provider(
    registry: Registry,
) -> Result<SdkMeterProvider, MetricsError> {
    let reader = opentelemetry_prometheus::exporter()
        .with_registry(registry)
        .build()?;
    let meter_provider = SdkMeterProvider::builder()
        .with_reader(reader)
        .with_view(CustomView::new()?)
        .with_resource(resource())
        .build();
    Ok(meter_provider)
}

#[cfg(feature = "prometheus")]
async fn prometheus_metrics_server(
    registry: Registry,
    host: IpAddr,
    port: u16,
) -> Result<(JoinHandle<()>, u16), Error> {
    let router = trillium_prometheus::text_format_handler(registry.clone());

    let (sender, receiver) = oneshot::channel();
    let init = Init::new(|info: Info| async move {
        // Ignore error if the receiver is dropped.
        let _ = sender.send(info.tcp_socket_addr().map(|socket_addr| socket_addr.port()));
    });

    let handle = tokio::task::spawn(
        trillium_tokio::config()
            .with_port(port)
            .with_host(&host.to_string())
            .without_signals()
            .run_async((init, router)),
    );

    let port = receiver
        .await
        .context("Init handler was dropped before sending port")?
        .context("server does not have a TCP port")?;

    Ok((handle, port))
}

/// Install a metrics provider and exporter, per the given configuration. The OpenTelemetry global
/// API can be used to create and update meters, and they will be sent through this exporter. The
/// returned handle should not be dropped until the application shuts down.
pub async fn install_metrics_exporter(
    config: &MetricsConfiguration,
) -> Result<MetricsExporterHandle, Error> {
    match &config.exporter {
        #[cfg(feature = "prometheus")]
        Some(MetricsExporterConfiguration::Prometheus {
            host: config_exporter_host,
            port: config_exporter_port,
        }) => {
            let registry = Registry::new();
            let meter_provider = build_opentelemetry_prometheus_meter_provider(registry.clone())?;
            set_meter_provider(meter_provider);

            let host = config_exporter_host
                .as_ref()
                .map(|host| IpAddr::from_str(host))
                .unwrap_or_else(|| Ok(Ipv4Addr::UNSPECIFIED.into()))?;
            let config_port = config_exporter_port.unwrap_or_else(|| 9464);

            let (handle, actual_port) =
                prometheus_metrics_server(registry, host, config_port).await?;

            Ok(MetricsExporterHandle::Prometheus {
                handle,
                port: actual_port,
            })
        }
        #[cfg(not(feature = "prometheus"))]
        Some(MetricsExporterConfiguration::Prometheus { .. }) => Err(Error::Other(anyhow!(
            "The OpenTelemetry Prometheus metrics exporter was enabled in the configuration file, \
             but support was not enabled at compile time. Rebuild with `--features prometheus`.",
        ))),

        #[cfg(feature = "otlp")]
        Some(MetricsExporterConfiguration::Otlp(otlp_config)) => {
            let exporter = opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint(otlp_config.endpoint.clone())
                .build_metrics_exporter(
                    Box::new(DefaultAggregationSelector::new()),
                    Box::new(DefaultTemporalitySelector::new()),
                )?;
            let reader = PeriodicReader::builder(exporter, Tokio).build();
            let meter_provider = SdkMeterProvider::builder()
                .with_reader(reader)
                .with_view(CustomView::new()?)
                .with_resource(resource())
                .build();
            // We can't drop the PushController, as that would stop pushes, so return it to the
            // caller.
            Ok(MetricsExporterHandle::Otlp(meter_provider))
        }
        #[cfg(not(feature = "otlp"))]
        Some(MetricsExporterConfiguration::Otlp(_)) => Err(Error::Other(anyhow!(
            "The OpenTelemetry OTLP metrics exporter was enabled in the configuration file, but \
             support was not enabled at compile time. Rebuild with `--features otlp`.",
        ))),

        // If neither exporter is configured, leave the default NoopMeterProvider in place.
        None => Ok(MetricsExporterHandle::Noop),
    }
}

/// Produces a [`opentelemetry::sdk::Resource`] representing this process.
#[cfg(any(feature = "otlp", feature = "prometheus"))]
fn resource() -> Resource {
    // Note that the implementation of `Default` pulls in attributes set via environment variables.
    let default_resource = Resource::default();

    let version_info_resource = Resource::new([
        KeyValue::new(
            "service.version",
            format!("{}-{}", env!("CARGO_PKG_VERSION"), git_revision()),
        ),
        KeyValue::new("process.runtime.name", "Rust"),
        KeyValue::new("process.runtime.version", env!("RUSTC_SEMVER")),
    ]);

    version_info_resource.merge(&default_resource)
}

pub(crate) fn report_aggregation_success_counter(meter: &Meter) -> Counter<u64> {
    let report_aggregation_success_counter = meter
        .u64_counter("janus_report_aggregation_success_counter")
        .with_description("Number of successfully-aggregated report shares")
        .with_unit(Unit::new("{report}"))
        .init();
    report_aggregation_success_counter.add(0, &[]);
    report_aggregation_success_counter
}

pub(crate) fn aggregate_step_failure_counter(meter: &Meter) -> Counter<u64> {
    let aggregate_step_failure_counter = meter
        .u64_counter("janus_step_failures")
        .with_description(concat!(
            "Failures while stepping aggregation jobs; these failures are ",
            "related to individual client reports rather than entire aggregation jobs."
        ))
        .with_unit(Unit::new("{error}"))
        .init();

    // Initialize counters with desired status labels. This causes Prometheus to see the first
    // non-zero value we record.
    for failure_type in [
        "missing_prepare_message",
        "missing_leader_input_share",
        "missing_helper_input_share",
        "prepare_init_failure",
        "prepare_step_failure",
        "prepare_message_failure",
        "unknown_hpke_config_id",
        "decrypt_failure",
        "input_share_decode_failure",
        "input_share_aad_encode_failure",
        "public_share_decode_failure",
        "public_share_encode_failure",
        "prepare_message_decode_failure",
        "leader_prep_share_decode_failure",
        "helper_prep_share_decode_failure",
        "continue_mismatch",
        "accumulate_failure",
        "finish_mismatch",
        "helper_step_failure",
        "plaintext_input_share_decode_failure",
        "duplicate_extension",
        "missing_client_report",
        "missing_prepare_message",
        "missing_or_malformed_taskprov_extension",
        "unexpected_taskprov_extension",
    ] {
        aggregate_step_failure_counter.add(0, &[KeyValue::new("type", failure_type)]);
    }

    aggregate_step_failure_counter
}
