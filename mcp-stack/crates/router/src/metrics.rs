use std::time::Duration;

use metrics::{counter, describe_counter, describe_histogram, histogram};

pub const RPC_COUNTER: &str = "mcp_router_rpc_total";
pub const RPC_LATENCY: &str = "mcp_router_rpc_latency_ms";
pub const RPC_BYTES_IN: &str = "mcp_router_rpc_bytes_in";
pub const RPC_BYTES_OUT: &str = "mcp_router_rpc_bytes_out";
pub const PROVIDER_USAGE: &str = "mcp_router_provider_usage";

pub fn init_metrics() {
    describe_counter!(
        RPC_COUNTER,
        "Total MCP RPC invocations by method and status"
    );
    describe_histogram!(RPC_LATENCY, "Latency of MCP RPC calls in milliseconds");
    describe_counter!(RPC_BYTES_IN, "Total bytes received per RPC method");
    describe_counter!(RPC_BYTES_OUT, "Total bytes sent per RPC method");
    describe_counter!(PROVIDER_USAGE, "Per-provider token usage");
}

pub fn record_rpc(
    method: &str,
    status: &str,
    duration: Duration,
    bytes_in: usize,
    bytes_out: usize,
) {
    counter!(RPC_COUNTER, 1, "method" => method.to_string(), "status" => status.to_string());
    histogram!(RPC_LATENCY, duration.as_millis() as f64, "method" => method.to_string(), "status" => status.to_string());
    counter!(RPC_BYTES_IN, bytes_in as u64, "method" => method.to_string(), "status" => status.to_string());
    counter!(RPC_BYTES_OUT, bytes_out as u64, "method" => method.to_string(), "status" => status.to_string());
}

pub fn record_provider_usage(provider: &str, tokens: i64, outcome: &str) {
    counter!(PROVIDER_USAGE, tokens as u64, "provider" => provider.to_string(), "outcome" => outcome.to_string());
}
