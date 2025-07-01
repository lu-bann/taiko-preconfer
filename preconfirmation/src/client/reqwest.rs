use alloy_consensus::Header;
use alloy_rpc_types_eth::Block;
use serde::{Deserialize, Serialize};

use crate::util::u64_to_hex;

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

pub async fn get_block_by_id(url: String, id: Option<u64>) -> Result<Block, reqwest::Error> {
    let params = if let Some(id) = id {
        serde_json::json!([u64_to_hex(id), true])
    } else {
        serde_json::json!(["latest", true])
    };
    let request = JsonRequest::new("eth_getBlockByNumber".into(), params);
    let response: JsonResponse<Block> = reqwest::Client::new()
        .get(&url)
        .json(&request)
        .send()
        .await
        .unwrap()
        .json()
        .await?;
    Ok(response.result)
}

pub async fn get_latest_header(url: String) -> Result<Header, reqwest::Error> {
    let request = JsonRequest::new(
        "eth_getBlockByNumber".into(),
        serde_json::json!(["latest", false]),
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

pub async fn get_latest_block(url: String) -> Result<Block, reqwest::Error> {
    let request = JsonRequest::new(
        "eth_getBlockByNumber".into(),
        serde_json::json!(["latest", true]),
    );
    let response: JsonResponse<Block> = reqwest::Client::new()
        .get(&url)
        .json(&request)
        .send()
        .await
        .unwrap()
        .json()
        .await?;
    Ok(response.result)
}
