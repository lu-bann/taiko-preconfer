use alloy_primitives::B256;
use serde::{Deserialize, Serialize};

use crate::client::HttpError;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SequencingStatus {
    #[serde(rename = "highestUnsafeL2PayloadBlockID")]
    pub highest_unsafe_l2_payload_block_id: u64,
    pub end_of_sequencing_block_hash: B256,
}

#[cfg_attr(test, mockall::automock)]
pub trait SequencingMonitor {
    fn ready(&self) -> impl Future<Output = Result<(), HttpError>>;
}

pub struct DummySequencingMonitor {}

impl SequencingMonitor for DummySequencingMonitor {
    async fn ready(&self) -> Result<(), HttpError> {
        Ok(())
    }
}
