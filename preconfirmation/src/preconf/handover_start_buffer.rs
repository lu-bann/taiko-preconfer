use std::{sync::Arc, time::Duration};

use alloy_consensus::Header;
use alloy_primitives::B256;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SequencingStatus {
    #[serde(rename = "highestUnsafeL2PayloadBlockID")]
    pub highest_unsafe_l2_payload_block_id: u64,
    pub end_of_sequencing_block_hash: B256,
}

#[cfg_attr(test, mockall::automock)]
pub trait SequencingMonitor {
    fn ready(&self) -> impl Future<Output = Result<(), reqwest::Error>>;
}

pub struct TaikoSequencingMonitor {
    last_header: Arc<Mutex<Option<Header>>>,
    url: String,
    poll_period: Duration,
}

impl TaikoSequencingMonitor {
    pub const fn new(
        last_header: Arc<Mutex<Option<Header>>>,
        url: String,
        poll_period: Duration,
    ) -> Self {
        Self {
            last_header,
            url,
            poll_period,
        }
    }
}

impl SequencingMonitor for TaikoSequencingMonitor {
    async fn ready(&self) -> Result<(), reqwest::Error> {
        loop {
            if let Some(last_header) = self.last_header.lock().await.clone() {
                let status: SequencingStatus = reqwest::get(&self.url).await?.json().await?;
                if status.end_of_sequencing_block_hash == last_header.hash_slow()
                    && status.highest_unsafe_l2_payload_block_id == last_header.number
                {
                    return Ok(());
                }
            }
            tokio::time::sleep(self.poll_period).await;
        }
    }
}
