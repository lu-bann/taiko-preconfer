use alloy_consensus::TxEnvelope;
use alloy_primitives::{Address, B256, Bytes, FixedBytes};
use alloy_rlp::RlpEncodable;
use alloy_rpc_types::Header;
use libdeflater::CompressionError;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{compression::compress, util::pad_left};

pub const PRECONF_BLOCKS: &str = "preconfBlocks";

#[derive(Debug, Serialize, RlpEncodable)]
#[serde(rename_all = "camelCase")]
pub struct ExecutableData {
    parent_hash: B256,
    fee_recipient: Address,
    block_number: u64,
    gas_limit: u64,
    timestamp: u64,
    transactions: Bytes,
    extra_data: Bytes,
    base_fee_per_gas: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildPreconfBlockRequest {
    pub executable_data: ExecutableData,
    pub end_of_sequencing: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildPreconfBlockResponse {
    pub block_header: Header,
}

#[allow(clippy::too_many_arguments)]
pub fn create_executable_data(
    base_fee_per_gas: u64,
    block_number: u64,
    sharing_percentage: u8,
    fee_recipient: Address,
    gas_limit: u64,
    parent_hash: FixedBytes<32>,
    timestamp: u64,
    txs: Vec<TxEnvelope>,
) -> Result<ExecutableData, CompressionError> {
    info!(
        "tx hash {}",
        alloy_primitives::keccak256(alloy_rlp::encode(&txs))
    );
    let tx_bytes = compress(txs)?;
    let transactions = Bytes::from(tx_bytes);

    Ok(ExecutableData {
        base_fee_per_gas,
        block_number,
        extra_data: pad_left::<32>(&[sharing_percentage]),
        fee_recipient,
        gas_limit,
        parent_hash,
        timestamp,
        transactions,
    })
}
