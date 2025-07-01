use alloy_consensus::TxEnvelope;
use alloy_json_rpc::RpcError;
use alloy_primitives::{Address, Bytes};
use alloy_rpc_client::RpcClient;
use alloy_rpc_types_engine::JwtSecret;
use alloy_transport::TransportErrorKind;
use alloy_transport_http::{
    AuthLayer, Http, HyperClient,
    hyper_util::{client::legacy::Client, rt::TokioExecutor},
};
use http_body_util::Full;
use serde::Deserialize;
use serde_json::json;
use tower::ServiceBuilder;
use url::Url;

pub const TAIKO_TX_POOL_CONTENT: &str = "taikoAuth_txPoolContent";
pub const TAIKO_TX_POOL_CONTENT_WITH_MIN_TIP: &str = "taikoAuth_txPoolContentWithMinTip";

#[derive(Debug, Deserialize)]
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
    let mempool_tx_lists: Option<Vec<MempoolTxList>> = client
        .request(TAIKO_TX_POOL_CONTENT.to_string(), params)
        .await?;
    Ok(mempool_tx_lists.unwrap_or_default())
}

#[allow(clippy::too_many_arguments)]
pub async fn get_mempool_tx_with_min_tip(
    client: &RpcClient,
    beneficiary: Address,
    base_fee: u64,
    block_max_gas_limit: u64,
    max_bytes_per_tx_list: u64,
    locals: Vec<String>,
    max_transactions_lists: u64,
    min_tip: u64,
) -> Result<Vec<MempoolTxList>, RpcError<TransportErrorKind>> {
    let params = json!([
        beneficiary,
        base_fee,
        block_max_gas_limit,
        max_bytes_per_tx_list,
        locals,
        max_transactions_lists,
        min_tip
    ]);
    client
        .request(TAIKO_TX_POOL_CONTENT_WITH_MIN_TIP.to_string(), params)
        .await
}

pub fn flatten_mempool_txs(tx_lists: Vec<MempoolTxList>) -> Vec<TxEnvelope> {
    tx_lists
        .into_iter()
        .flat_map(|tx_list| tx_list.tx_list)
        .collect()
}

pub fn get_alloy_auth_client(
    url: &str,
    jwt_secret: JwtSecret,
    is_local: bool,
) -> Result<RpcClient, url::ParseError> {
    let hyper_client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();
    let auth_layer = AuthLayer::new(jwt_secret);
    let service = ServiceBuilder::new()
        .layer(auth_layer)
        .service(hyper_client);

    let layer_transport = HyperClient::with_service(service);
    let transport = Http::with_client(layer_transport, Url::parse(url)?);
    Ok(RpcClient::new(transport, is_local))
}
