use mcp_router::{jsonrpc, util};

#[test]
fn method_not_found_has_code() {
    let response = jsonrpc::method_not_found("unknown");
    assert!(response.error.is_some());
    assert_eq!(response.jsonrpc, "2.0");
}

#[test]
fn resource_uri_round_trip() {
    let encoded = util::encode_resource_uri("srv", "file:///tmp/test.txt");
    let (srv, inner) = util::decode_resource_uri(&encoded).expect("decode");
    assert_eq!(srv, "srv");
    assert_eq!(inner, "file:///tmp/test.txt");
}
