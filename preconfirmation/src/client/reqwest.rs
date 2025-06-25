use alloy_consensus::Header;
use serde::{Deserialize, Serialize};

use crate::encode_util::u64_to_hex;

#[derive(Debug, Serialize)]
pub struct JsonRequest {
    jsonrpc: String,
    method: String,
    params: serde_json::Value,
    id: u64,
}

impl JsonRequest {
    pub fn new(method: String, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            method,
            params,
            id: 1,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct JsonResponse<T> {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: u64,
    result: T,
}

pub async fn get_header_by_id(url: String, id: u64) -> Result<Header, reqwest::Error> {
    let request = JsonRequest::new(
        "eth_getBlockByNumber".into(),
        serde_json::json!([u64_to_hex(id), false]),
    );
    let response: JsonResponse<Header> = reqwest::Client::new()
        .get(&url)
        .json(&request)
        .send()
        .await
        .unwrap()
        .json()
        .await?;
    Ok(response.result)
}
