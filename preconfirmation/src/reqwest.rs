use alloy_consensus::Header;
use alloy_json_rpc::{Request, Response, ResponsePayload};
use alloy_rpc_types_eth::Block;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;

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

#[derive(Debug, Error)]
pub enum RpcError {
    #[error("{0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("{0}")]
    Rpc(#[from] alloy_json_rpc::RpcError<alloy_transport::TransportErrorKind>),
}

pub async fn auth_reqwest<Req: Serialize, Resp: DeserializeOwned>(
    url: &str,
    request: &Req,
    jwt_secret: String,
) -> Result<Resp, RpcError> {
    let request_builder = reqwest::Client::new()
        .post(url)
        .header("Authorization", jwt_secret);
    Ok(request_builder.json(&request).send().await?.json().await?)
}

pub async fn json_auth_reqwest<Resp: DeserializeOwned>(
    url: &str,
    method: String,
    params: serde_json::Value,
    jwt_secret: String,
) -> Result<Resp, RpcError> {
    let request = Request::new(method, alloy_json_rpc::Id::Number(1), params);

    let response: Response<Resp> = auth_reqwest(url, &request, jwt_secret).await?;

    match response.payload {
        ResponsePayload::Success(payload) => Ok(payload),
        ResponsePayload::Failure(error_payload) => Err(RpcError::Rpc(
            alloy_json_rpc::RpcError::ErrorResp(error_payload),
        )),
    }
}
