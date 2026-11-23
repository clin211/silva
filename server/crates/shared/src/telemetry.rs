use anyhow::Context as _;
use opentelemetry::{
    KeyValue,
    propagation::TextMapPropagator,
    trace::{TraceContextExt, TracerProvider},
};
use opentelemetry_appender_tracing::layer::{OpenTelemetryTracingBridge, TracingSpanAttributes};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    Resource,
    logs::SdkLoggerProvider,
    propagation::TraceContextPropagator,
    trace::{Sampler, SdkTracerProvider},
};
use std::{collections::HashMap, env, time::Duration};
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::{
    EnvFilter, Layer, filter::FilterExt, layer::SubscriberExt, util::SubscriberInitExt,
};

pub struct TelemetryGuard {
    traces: SdkTracerProvider,
    logs: Option<SdkLoggerProvider>,
}

impl TelemetryGuard {
    /// SDK 使用独立线程导出；退出前将阻塞式 flush 移出 Tokio 工作线程。
    pub async fn shutdown(self) {
        if let Err(error) = tokio::task::spawn_blocking(move || drop(self)).await {
            tracing::error!(%error, "telemetry shutdown task failed");
        }
    }
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Some(logs) = &self.logs
            && let Err(error) = logs.shutdown()
        {
            tracing::warn!(%error, "telemetry logs shutdown failed");
        }
        if let Err(error) = self.traces.shutdown() {
            tracing::warn!(%error, "telemetry traces shutdown failed");
        }
    }
}

pub fn init(service: &'static str) -> anyhow::Result<TelemetryGuard> {
    configure(service, true)
}

/// 仅输出本地 JSON 日志并生成链路 ID，供隔离测试与离线诊断使用。
pub fn init_local(service: &'static str) -> anyhow::Result<TelemetryGuard> {
    configure(service, false)
}

fn configure(service: &'static str, allow_export: bool) -> anyhow::Result<TelemetryGuard> {
    let ratio: f64 = env::var("OTEL_TRACES_SAMPLER_ARG")
        .unwrap_or_else(|_| "1".into())
        .parse()
        .context("invalid OTEL_TRACES_SAMPLER_ARG")?;
    anyhow::ensure!(
        ratio.is_finite() && (0.0..=1.0).contains(&ratio),
        "OTEL_TRACES_SAMPLER_ARG must be between 0 and 1"
    );
    let environment = env::var("SILVA_ENV").unwrap_or_else(|_| "development".into());
    let resource = Resource::builder()
        .with_service_name(service)
        .with_attribute(KeyValue::new("service.version", env!("CARGO_PKG_VERSION")))
        .with_attribute(KeyValue::new(
            "deployment.environment.name",
            environment.clone(),
        ))
        .build();
    let export = env::var_os("OTEL_EXPORTER_OTLP_ENDPOINT").is_some();
    let mut traces = SdkTracerProvider::builder()
        .with_resource(resource.clone())
        .with_sampler(Sampler::ParentBased(Box::new(Sampler::TraceIdRatioBased(
            ratio,
        ))));
    if allow_export && (export || env::var_os("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT").is_some()) {
        traces = traces.with_batch_exporter(
            opentelemetry_otlp::SpanExporter::builder()
                .with_http()
                .with_timeout(Duration::from_secs(3))
                .build()?,
        );
    }
    let traces = traces.build();
    let logs =
        if allow_export && (export || env::var_os("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT").is_some()) {
            Some(
                SdkLoggerProvider::builder()
                    .with_resource(resource)
                    .with_batch_exporter(
                        opentelemetry_otlp::LogExporter::builder()
                            .with_http()
                            .with_timeout(Duration::from_secs(3))
                            .build()?,
                    )
                    .build(),
            )
        } else {
            None
        };
    let bridge = logs.as_ref().map(|provider| {
        OpenTelemetryTracingBridge::builder(provider)
            .with_tracing_span_attributes(TracingSpanAttributes::allowlist([
                "request_id",
                "route",
                "method",
                "job_id",
                "event_id",
                "generation",
            ]))
            .build()
            // 排除导出器自己的日志，防止导出失败触发递归上报。
            .with_filter(tracing_subscriber::filter::filter_fn(|meta| {
                !["opentelemetry", "reqwest", "hyper", "tonic"]
                    .iter()
                    .any(|prefix| meta.target().starts_with(prefix))
            }))
    });
    let filter = match env::var("RUST_LOG") {
        Ok(value) => EnvFilter::try_new(value).context("invalid RUST_LOG")?,
        Err(_) => EnvFilter::new("info"),
    };
    tracing_subscriber::registry()
        // 日志等级不会关闭请求 span，RUST_LOG=warn 时仍能取得有效 trace_id。
        .with(
            tracing_opentelemetry::layer()
                .with_tracer(traces.tracer(service))
                .with_filter(tracing_subscriber::filter::filter_fn(|meta| {
                    meta.target().starts_with("silva")
                })),
        )
        .with(bridge.map(|layer| {
            layer.with_filter(
                filter
                    .clone()
                    .or(tracing_subscriber::filter::filter_fn(|meta| {
                        meta.is_span() && meta.target().starts_with("silva")
                    })),
            )
        }))
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_current_span(true)
                .with_span_list(true)
                .with_filter(filter.or(tracing_subscriber::filter::filter_fn(|meta| {
                    meta.is_span() && meta.target().starts_with("silva")
                }))),
        )
        .try_init()
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    tracing::info!(service, version = env!("CARGO_PKG_VERSION"), %environment, "telemetry initialized");
    Ok(TelemetryGuard { traces, logs })
}

/// 只传播 W3C Trace Context，不接受客户端 baggage 或请求 ID。
pub fn set_parent(span: &tracing::Span, headers: &HashMap<String, String>) {
    let parent = TraceContextPropagator::new().extract(headers);
    if let Err(error) = span.set_parent(parent) {
        tracing::debug!(%error, "trace parent could not be attached");
    }
}

pub fn trace_id(span: &tracing::Span) -> String {
    span.context().span().span_context().trace_id().to_string()
}

/// 供外部 HTTP 调用和后续 Outbox 生产者保存追踪上下文；不包含业务凭据。
pub fn inject_context() -> HashMap<String, String> {
    let mut headers = HashMap::new();
    TraceContextPropagator::new().inject_context(&tracing::Span::current().context(), &mut headers);
    headers
}
