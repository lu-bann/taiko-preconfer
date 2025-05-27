#![allow(dead_code)]

use alloy_consensus::{Header, TxEnvelope};
use alloy_primitives::{Address, Bytes};
use alloy_rpc_client::RpcClient as AlloyClient;
use alloy_rpc_types::BlockNumberOrTag;
use alloy_rpc_types_engine::JwtSecret;
use alloy_transport_http::{
    AuthLayer, Http, HyperClient,
    hyper_util::{client::legacy::Client, rt::TokioExecutor},
};
use block_building::http_client::{HttpClient, HttpError};
use http_body_util::Full;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower::ServiceBuilder;
use url::Url;

use crate::error::{PreconferError, PreconferResult};

const GET_BLOCK_BY_NUMBER: &str = "eth_getBlockByNumber";
const GET_HEADER_BY_NUMBER: &str = "eth_getHeaderByNumber";
const TAIKO_TX_POOL_CONTENT: &str = "taikoAuth_txPoolContent";
const TAIKO_TX_POOL_CONTENT_WITH_MIN_TIP: &str = "taikoAuth_txPoolContentWithMinTip";

pub fn get_client(url: &str) -> PreconferResult<AlloyClient> {
    let transport = Http::new(Url::parse(url)?);
    Ok(AlloyClient::new(transport, false))
}

pub fn get_auth_client(url: &str, jwt_secret: JwtSecret) -> PreconferResult<AlloyClient> {
    let hyper_client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();
    let auth_layer = AuthLayer::new(jwt_secret);
    let service = ServiceBuilder::new().layer(auth_layer).service(hyper_client);

    let layer_transport = HyperClient::with_service(service);
    let http_hyper = Http::with_client(layer_transport, Url::parse(url)?);

    Ok(AlloyClient::new(http_hyper, true))
}

pub async fn get_header<Client: HttpClient>(
    client: &Client,
    block_number: BlockNumberOrTag,
) -> PreconferResult<Header> {
    let params = json!([block_number]);

    let header: Option<Header> =
        client.request(GET_HEADER_BY_NUMBER.to_string(), params.clone()).await?;
    header.ok_or(PreconferError::FailedRPCRequest {
        method: GET_HEADER_BY_NUMBER.to_string(),
        params,
    })
}

pub async fn get_header_by_id<Client: HttpClient>(
    client: &Client,
    id: u64,
) -> PreconferResult<Header> {
    get_header(client, BlockNumberOrTag::Number(id)).await
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MempoolTxList {
    pub tx_list: Vec<TxEnvelope>,
    pub estimated_gas_used: u64,
    pub bytes_length: u64,
}

pub async fn get_mempool_txs<Client: HttpClient>(
    client: &Client,
    beneficiary: Address,
    base_fee: u64,
    block_max_gas_limit: u64,
    max_bytes_per_tx_list: u64,
    locals: Vec<String>,
    max_transactions_lists: u64,
) -> Result<Vec<MempoolTxList>, HttpError> {
    let params = json!([
        beneficiary,
        base_fee,
        block_max_gas_limit,
        max_bytes_per_tx_list,
        locals,
        max_transactions_lists
    ]);
    client.request(TAIKO_TX_POOL_CONTENT.to_string(), params).await
}

#[allow(clippy::too_many_arguments)]
pub async fn get_mempool_tx_with_min_tip<Client: HttpClient>(
    client: &Client,
    beneficiary: Address,
    base_fee: u64,
    block_max_gas_limit: u64,
    max_bytes_per_tx_list: u64,
    locals: Vec<String>,
    max_transactions_lists: u64,
    min_tip: u64,
) -> Result<Vec<MempoolTxList>, HttpError> {
    let params = json!([
        beneficiary,
        base_fee,
        block_max_gas_limit,
        max_bytes_per_tx_list,
        locals,
        max_transactions_lists,
        min_tip
    ]);
    client.request(TAIKO_TX_POOL_CONTENT_WITH_MIN_TIP.to_string(), params).await
}

pub fn flatten_mempool_txs(tx_lists: Vec<MempoolTxList>) -> Vec<TxEnvelope> {
    tx_lists.into_iter().flat_map(|tx_list| tx_list.tx_list).collect()
}
