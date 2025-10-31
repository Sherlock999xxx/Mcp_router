use mcp_router::jsonrpc;
use mcp_router::config::Config;

#[test]
fn method_not_found_has_code() {
    let response = jsonrpc::method_not_found("unknown");
    assert!(response.error.is_some());
    assert_eq!(response.jsonrpc, "2.0");
}

#[test]
fn config_default_has_database() {
    let cfg = Config::default();
    assert_eq!(cfg.database.path, "sqlite://mcp-router.db");
    assert!(cfg.upstreams.contains_key("fs"));
}
