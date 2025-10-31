use mcp_router::jsonrpc;

#[test]
fn method_not_found_has_code() {
    let response = jsonrpc::method_not_found("unknown");
    assert!(response.error.is_some());
    assert_eq!(response.jsonrpc, "2.0");
}
