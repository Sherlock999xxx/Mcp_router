use base64::{engine::general_purpose::STANDARD as BASE64, Engine};

pub fn encode_resource_uri(server: &str, upstream_uri: &str) -> String {
    let encoded = BASE64.encode(upstream_uri.as_bytes());
    format!("mcp+router://{}/{}", server, encoded)
}

pub fn decode_resource_uri(uri: &str) -> Option<(String, String)> {
    let uri = uri.strip_prefix("mcp+router://")?;
    let (server, encoded) = uri.split_once('/')?;
    let decoded = BASE64.decode(encoded.as_bytes()).ok()?;
    let upstream_uri = String::from_utf8(decoded).ok()?;
    Some((server.to_string(), upstream_uri))
}
