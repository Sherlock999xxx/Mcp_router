use std::sync::Arc;

use prometheus::{Encoder, HistogramVec, IntCounterVec, IntGauge, Registry, TextEncoder};

#[derive(Clone)]
pub struct MetricsHandle {
    registry: Registry,
    rpc_calls: IntCounterVec,
    rpc_latency: HistogramVec,
    #[allow(dead_code)]
    usage_tokens: IntCounterVec,
    #[allow(dead_code)]
    usage_errors: IntCounterVec,
    #[allow(dead_code)]
    active_sessions: Arc<IntGauge>,
}

impl MetricsHandle {
    pub fn new() -> Self {
        let registry = Registry::new();
        let rpc_calls = IntCounterVec::new(
            prometheus::Opts::new("mcp_router_rpc_calls", "Total RPC calls"),
            &["method", "status"],
        )
        .expect("rpc counter");
        let rpc_latency = HistogramVec::new(
            prometheus::HistogramOpts::new("mcp_router_rpc_latency_seconds", "RPC latency"),
            &["method"],
        )
        .expect("rpc latency");
        let usage_tokens = IntCounterVec::new(
            prometheus::Opts::new("mcp_router_usage_tokens", "Token usage per provider"),
            &["provider"],
        )
        .expect("token counter");
        let usage_errors = IntCounterVec::new(
            prometheus::Opts::new("mcp_router_usage_errors", "Errors per provider"),
            &["provider"],
        )
        .expect("error counter");
        let active_sessions =
            IntGauge::new("mcp_router_active_sessions", "Active MCP sessions").expect("gauge");

        registry.register(Box::new(rpc_calls.clone())).ok();
        registry.register(Box::new(rpc_latency.clone())).ok();
        registry.register(Box::new(usage_tokens.clone())).ok();
        registry.register(Box::new(usage_errors.clone())).ok();
        registry.register(Box::new(active_sessions.clone())).ok();

        Self {
            registry,
            rpc_calls,
            rpc_latency,
            usage_tokens,
            usage_errors,
            active_sessions: Arc::new(active_sessions),
        }
    }

    pub fn render(&self) -> axum::response::Response {
        let mut buffer = Vec::new();
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        axum::response::Response::builder()
            .header(axum::http::header::CONTENT_TYPE, encoder.format_type())
            .body(axum::body::Body::from(buffer))
            .unwrap()
    }

    pub fn record_call(&self, method: &str, status: &str) {
        self.rpc_calls.with_label_values(&[method, status]).inc();
    }

    pub fn observe_latency(&self, method: &str, seconds: f64) {
        self.rpc_latency
            .with_label_values(&[method])
            .observe(seconds);
    }

    #[allow(dead_code)]
    pub fn record_tokens(&self, provider: &str, tokens: u64) {
        self.usage_tokens
            .with_label_values(&[provider])
            .inc_by(tokens as u64);
    }

    #[allow(dead_code)]
    pub fn record_error(&self, provider: &str) {
        self.usage_errors.with_label_values(&[provider]).inc();
    }

    #[allow(dead_code)]
    pub fn active_sessions(&self) -> Arc<IntGauge> {
        self.active_sessions.clone()
    }
}
