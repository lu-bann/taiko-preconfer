use std::{future::Future, num::ParseIntError};

use alloy_consensus::{Header, TxEnvelope};
use alloy_json_rpc::{RpcError, RpcRecv};
use alloy_primitives::Address;
use alloy_rpc_client::RpcClientInner;
use alloy_rpc_types::{Block, BlockNumberOrTag};
use alloy_transport::TransportErrorKind;
#[cfg(test)]
use mockall::automock;
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use thiserror::Error;

use crate::encode_util::hex_to_u64;

pub const GET_BLOCK_BY_NUMBER: &str = "eth_getBlockByNumber";
pub const GET_HEADER_BY_NUMBER: &str = "eth_getHeaderByNumber";
pub const GET_TRANSACTION_COUNT: &str = "eth_getTransactionCount";
pub const TAIKO_TX_POOL_CONTENT: &str = "taikoAuth_txPoolContent";
pub const TAIKO_TX_POOL_CONTENT_WITH_MIN_TIP: &str = "taikoAuth_txPoolContentWithMinTip";

#[derive(Error, Debug)]
pub enum HttpError {
    #[error("{0}")]
    Rpc(#[from] RpcError<TransportErrorKind>),

    #[error("{0}")]
    ParseInt(#[from] ParseIntError),

    #[error("RPC request {method} with {params} failed.")]
    FailedRPCRequest { method: String, params: JsonValue },
}

#[cfg_attr(test, automock)]
pub trait HttpClient {
    fn request<Resp: RpcRecv>(
        &self,
        method: String,
        params: serde_json::Value,
    ) -> impl Future<Output = Result<Resp, HttpError>>;
}

pub async fn get_nonce(client: &RpcClientInner, address: &str) -> Result<u64, HttpError> {
    let params = json!([address, "pending"]);
    let transaction_count_hex_str: String = client
        .request(GET_TRANSACTION_COUNT.to_string(), params)
        .await?;
    Ok(hex_to_u64(&transaction_count_hex_str)?)
}

pub async fn get_header(
    client: &RpcClientInner,
    block_number: BlockNumberOrTag,
) -> Result<Header, HttpError> {
    let params = json!([block_number]);

    let header: Option<Header> = client
        .request(GET_HEADER_BY_NUMBER.to_string(), params.clone())
        .await?;
    header.ok_or(HttpError::FailedRPCRequest {
        method: GET_HEADER_BY_NUMBER.to_string(),
        params,
    })
}

pub async fn get_header_by_id(client: &RpcClientInner, id: u64) -> Result<Header, HttpError> {
    get_header(client, BlockNumberOrTag::Number(id)).await
}

pub async fn get_latest_header(client: &RpcClientInner) -> Result<Header, HttpError> {
    get_header(client, BlockNumberOrTag::Latest).await
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
    let mempool_tx_lists: Option<Vec<MempoolTxList>> = client
        .request(TAIKO_TX_POOL_CONTENT.to_string(), params)
        .await?;
    Ok(mempool_tx_lists.unwrap_or_default())
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

pub async fn get_block<Client: HttpClient>(
    client: &Client,
    block_number: BlockNumberOrTag,
    full_tx: bool,
) -> Result<Block, HttpError> {
    let params = json!([block_number, full_tx]);
    let block: Option<Block> = client
        .request(GET_BLOCK_BY_NUMBER.to_string(), params.clone())
        .await?;
    block.ok_or(HttpError::Rpc(alloy_json_rpc::RpcError::NullResp))
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{B256, hex::FromHex};
    use alloy_rpc_types::BlockTransactions;

    use super::*;
    use crate::client::MockHttpClient;

    #[tokio::test]
    async fn test_get_block() {
        let mut client = MockHttpClient::new();

        let first_hash =
            B256::from_hex("0xb57c361f4e5fd7a2b6fc2246502c5fefd532217d0d5e7ffb75c841a7433914ad")
                .unwrap();
        let expected_first_hash = first_hash;
        let second_hash =
            B256::from_hex("0xc2a716e6782e1efa88f8f204eb202005cebe3f4e7b6109b3bd300367545ef69a")
                .unwrap();
        client.expect_request::<Block>().return_once(move |_, _| {
            let block_transactions = vec![first_hash, second_hash];
            Box::pin(async {
                Ok(Block::default()
                    .with_transactions(BlockTransactions::Hashes(block_transactions)))
            })
        });

        let method = GET_BLOCK_BY_NUMBER.to_string();
        let params = json!([BlockNumberOrTag::Latest, false]);
        let block: Block = client.request(method, params).await.unwrap();
        assert_eq!(
            block.transactions.hashes().next().unwrap(),
            expected_first_hash
        );
        assert_eq!(block.transactions.len(), 2);
    }
}
