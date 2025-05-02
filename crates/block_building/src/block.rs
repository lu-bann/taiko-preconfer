use alloy_primitives::{Address, B256, Bloom, Bytes};
use serde::{Deserialize, Serialize};

/// [`PreconfBlockRequestBody`] is the request body for the `/preconfBlock` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreconfBlockRequestBody {
    inner: ExecutableData,
}

impl PreconfBlockRequestBody {
    pub fn _new(inner: ExecutableData) -> Self {
        Self { inner }
    }

    pub fn _inner(&self) -> &ExecutableData {
        &self.inner
    }
}

/// [`ExecutableData`] is the data necessary to execute an EL payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutableData {
    pub parent_hash: B256,
    pub fee_recipient: Address,
    pub state_root: B256,
    pub receipts_root: B256,
    pub logs_bloom: Bloom,
    pub block_number: u64,
    pub gas_limit: u64,
    pub timestamp: u64,
    /// Transactions list with RLP encoded at first, then zlib compressed.
    pub transactions: Bytes,
    pub extra_data: Bytes,
    pub base_fee_per_gas: u64,
}
