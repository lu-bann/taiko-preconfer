use alloy_consensus::TxEnvelope;
use alloy_json_rpc::RpcError;
use alloy_primitives::{Address, Bytes};
use alloy_rpc_client::RpcClient;
use alloy_rpc_types::{Block, BlockNumberOrTag};
use alloy_rpc_types_engine::JwtSecret;
use alloy_transport::TransportErrorKind;
use alloy_transport_http::{
    AuthLayer, Http, HyperClient,
    hyper_util::{client::legacy::Client, rt::TokioExecutor},
};
use http_body_util::Full;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower::ServiceBuilder;
use url::Url;

use crate::error::{PreconferError, PreconferResult};

const GET_LATEST_BLOCK: &str = "eth_getBlockByNumber";
const TAIKO_TX_POOL_CONTENT: &str = "taikoAuth_txPoolContent";

pub fn get_client(url: &str) -> PreconferResult<RpcClient> {
    let transport = Http::new(Url::parse(url)?);
    Ok(RpcClient::new(transport, false))
}

pub fn get_auth_client(url: &str, jwt_secret: JwtSecret) -> PreconferResult<RpcClient> {
    let hyper_client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();
    let auth_layer = AuthLayer::new(jwt_secret);
    let service = ServiceBuilder::new().layer(auth_layer).service(hyper_client);

    let layer_transport = HyperClient::with_service(service);
    let http_hyper = Http::with_client(layer_transport, Url::parse(url)?);

    Ok(RpcClient::new(http_hyper, true))
}

pub async fn get_latest_block(client: &RpcClient) -> PreconferResult<Block> {
    let request_full_tx_objects = false;
    let params = json!([BlockNumberOrTag::Latest, request_full_tx_objects]);

    let rpc_call = client.request(GET_LATEST_BLOCK, params.clone());
    let method = rpc_call.method().to_string();
    let params = rpc_call.request().params.clone();
    let block: Option<Block> = rpc_call.await?;
    block.ok_or(PreconferError::FailedRPCRequest { method, params })
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MempoolTxList {
    pub tx_list: Vec<TxEnvelope>,
    pub estimated_gas_used: u64,
    pub bytes_length: u64,
}

pub async fn get_mempool_txs(
    client: &RpcClient,
    beneficiary: Address,
    base_fee: u64,
    block_max_gas_limit: u64,
    max_bytes_per_tx_list: u64,
    locals: Vec<String>,
    max_transactions_lists: u64,
) -> Result<Vec<MempoolTxList>, RpcError<TransportErrorKind>> {
    let params = json!([
        beneficiary,
        base_fee,
        block_max_gas_limit,
        max_bytes_per_tx_list,
        locals,
        max_transactions_lists
    ]);
    let rpc_call = client.request(TAIKO_TX_POOL_CONTENT, params);
    rpc_call.await
}
