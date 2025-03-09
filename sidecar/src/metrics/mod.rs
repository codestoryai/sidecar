use axum::{
    routing::get,
    Router,
    response::{IntoResponse, Response},
    http::{header, StatusCode},
    extract::State,
};
use prometheus::{Registry, Counter, Histogram, register_counter, register_histogram};
use opentelemetry::{
    trace::{Tracer, TracerProvider},
    global,
    sdk::{trace::TracerProvider as SdkTracerProvider, Resource},
};
use opentelemetry_jaeger::new_pipeline;
use std::sync::Arc;

pub struct MetricsState {
    registry: Registry,
    request_counter: Counter,
    error_counter: Counter,
    request_duration: Histogram,
    tracer: Arc<dyn Tracer>,
}

impl MetricsState {
    pub fn new() -> Arc<Self> {
        let registry = Registry::new();
        
        let request_counter = register_counter!(
            "sidecar_requests_total",
            "Total number of requests processed"
        ).unwrap();
        
        let error_counter = register_counter!(
            "sidecar_errors_total",
            "Total number of errors encountered"
        ).unwrap();
        
        let request_duration = register_histogram!(
            "sidecar_request_duration_seconds",
            "Request duration in seconds",
            vec![0.01, 0.05, 0.1, 0.5, 1.0, 5.0]
        ).unwrap();

        // Initialize OpenTelemetry tracer
        let tracer_provider = new_pipeline()
            .with_service_name("sidecar")
            .with_collector_endpoint("http://localhost:14268/api/traces")
            .build_simple()
            .unwrap();
        
        global::set_tracer_provider(tracer_provider.clone());
        let tracer = tracer_provider.tracer("sidecar");

        Arc::new(Self {
            registry,
            request_counter,
            error_counter,
            request_duration,
            tracer: Arc::new(tracer),
        })
    }

    pub fn increment_request(&self) {
        self.request_counter.inc();
    }

    pub fn increment_error(&self) {
        self.error_counter.inc();
    }

    pub fn observe_duration(&self, duration: f64) {
        self.request_duration.observe(duration);
    }
}

pub fn router() -> Router<Arc<MetricsState>> {
    Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/traces", get(traces_handler))
}

async fn metrics_handler(
    State(state): State<Arc<MetricsState>>
) -> Response {
    let mut buffer = Vec::new();
    let encoder = prometheus::TextEncoder::new();
    
    encoder.encode(&state.registry.gather(), &mut buffer)
        .unwrap_or_default();

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, prometheus::TEXT_FORMAT)
        .body(buffer.into())
        .unwrap()
        .into_response()
}

async fn traces_handler(
    State(state): State<Arc<MetricsState>>
) -> Response {
    let tracer = state.tracer.clone();
    let span = tracer.start("traces_handler");
    
    // Collect active traces
    let trace_data = span.span_context()
        .trace_id()
        .to_string();

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(trace_data.into())
        .unwrap()
        .into_response()
}