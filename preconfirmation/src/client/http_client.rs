use alloy_consensus::{Header, TxEnvelope};
use alloy_json_rpc::RpcError;
use alloy_primitives::Address;
use alloy_rpc_client::{RpcClient, RpcClientInner};
use alloy_rpc_types::{Block, BlockNumberOrTag};
use alloy_transport::TransportErrorKind;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const GET_BLOCK_BY_NUMBER: &str = "eth_getBlockByNumber";
pub const GET_HEADER_BY_NUMBER: &str = "eth_getHeaderByNumber";
pub const TAIKO_TX_POOL_CONTENT: &str = "taikoAuth_txPoolContent";
pub const TAIKO_TX_POOL_CONTENT_WITH_MIN_TIP: &str = "taikoAuth_txPoolContentWithMinTip";

pub async fn get_header(
    client: &RpcClientInner,
    block_number: BlockNumberOrTag,
) -> Result<Header, RpcError<TransportErrorKind>> {
    let params = json!([block_number]);

    let header: Option<Header> = client
        .request(GET_HEADER_BY_NUMBER.to_string(), params.clone())
        .await?;
    header.ok_or(RpcError::NullResp)
}

pub async fn get_header_by_id(
    client: &RpcClientInner,
    id: u64,
) -> Result<Header, RpcError<TransportErrorKind>> {
    get_header(client, BlockNumberOrTag::Number(id)).await
}

pub async fn get_latest_header(
    client: &RpcClientInner,
) -> Result<Header, RpcError<TransportErrorKind>> {
    get_header(client, BlockNumberOrTag::Latest).await
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

pub async fn get_block(
    client: &RpcClientInner,
    block_number: BlockNumberOrTag,
    full_tx: bool,
) -> Result<Block, RpcError<TransportErrorKind>> {
    let params = json!([block_number, full_tx]);
    let block: Option<Block> = client
        .request(GET_BLOCK_BY_NUMBER.to_string(), params.clone())
        .await?;
    block.ok_or(RpcError::NullResp)
}
