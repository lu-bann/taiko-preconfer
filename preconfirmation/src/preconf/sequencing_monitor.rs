use std::{sync::Arc, time::Duration};

use alloy_consensus::Header;
use alloy_primitives::B256;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::debug;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SequencingStatus {
    #[serde(rename = "highestUnsafeL2PayloadBlockID")]
    pub highest_unsafe_l2_payload_block_id: u64,
    pub end_of_sequencing_block_hash: B256,
}

#[cfg_attr(test, mockall::automock)]
pub trait IStatusMonitor {
    fn status(&self) -> impl Future<Output = Result<SequencingStatus, reqwest::Error>>;
}

pub struct TaikoStatusMonitor {
    url: String,
}

impl TaikoStatusMonitor {
    pub const fn new(url: String) -> Self {
        Self { url }
    }
}

impl IStatusMonitor for TaikoStatusMonitor {
    async fn status(&self) -> Result<SequencingStatus, reqwest::Error> {
        reqwest::get(&self.url).await?.json().await
    }
}

pub struct TaikoSequencingMonitor<StatusMonitor: IStatusMonitor> {
    last_header: Arc<RwLock<Header>>,
    poll_period: Duration,
    monitor: StatusMonitor,
}

impl<StatusMonitor: IStatusMonitor> TaikoSequencingMonitor<StatusMonitor> {
    pub const fn new(
        last_header: Arc<RwLock<Header>>,
        poll_period: Duration,
        monitor: StatusMonitor,
    ) -> Self {
        Self {
            last_header,
            poll_period,
            monitor,
        }
    }

    pub async fn ready(&self) -> Result<(), reqwest::Error> {
        loop {
            let status: SequencingStatus = self.monitor.status().await?;
            if is_end_of_sequencing_status(&status, &*self.last_header.read().await) {
                return Ok(());
            }
            debug!("Out of sync. status={:?} {:?}", status, self.last_header);
            tokio::time::sleep(self.poll_period).await;
        }
    }
}

fn is_end_of_sequencing_status(status: &SequencingStatus, header: &Header) -> bool {
    status.end_of_sequencing_block_hash == header.hash_slow()
        && status.highest_unsafe_l2_payload_block_id == header.number
}
#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use tokio::sync::RwLock;

    use crate::{
        preconf::sequencing_monitor::{
            MockIStatusMonitor, SequencingStatus, TaikoSequencingMonitor,
            is_end_of_sequencing_status,
        },
        test_util::get_header,
    };

    const TEST_DURATION: Duration = Duration::from_millis(1);

    #[test]
    fn when_hash_and_block_id_match_then_is_end_of_sequencing() {
        let block_number = 3;
        let timestamp = 42;
        let header = get_header(block_number, timestamp);
        let status = SequencingStatus {
            highest_unsafe_l2_payload_block_id: block_number,
            end_of_sequencing_block_hash: header.hash_slow(),
        };
        assert!(is_end_of_sequencing_status(&status, &header));
    }

    #[test]
    fn when_block_id_is_different_then_is_not_end_of_sequencing() {
        let block_number = 3;
        let highest_unsafe_l2_payload_block_id = 6;
        let timestamp = 42;
        let header = get_header(block_number, timestamp);
        let status = SequencingStatus {
            highest_unsafe_l2_payload_block_id,
            end_of_sequencing_block_hash: header.hash_slow(),
        };
        assert!(!is_end_of_sequencing_status(&status, &header));
    }

    #[test]
    fn when_hash_is_different_then_is_not_end_of_sequencing() {
        let block_number = 3;
        let timestamp = 42;
        let header = get_header(block_number, timestamp);
        let other_timestamp = 69;
        let end_of_sequencing_block_hash = get_header(block_number, other_timestamp).hash_slow();
        let status = SequencingStatus {
            highest_unsafe_l2_payload_block_id: block_number,
            end_of_sequencing_block_hash,
        };
        assert!(!is_end_of_sequencing_status(&status, &header));
    }

    #[tokio::test]
    async fn when_header_and_status_match_then_sequencing_monitor_does_return() {
        let block_number = 3;
        let timestamp = 42;
        let header = get_header(block_number, timestamp);
        let status = SequencingStatus {
            highest_unsafe_l2_payload_block_id: block_number,
            end_of_sequencing_block_hash: header.hash_slow(),
        };
        let header = Arc::new(RwLock::new(header));
        let mut status_monitor = MockIStatusMonitor::new();
        status_monitor
            .expect_status()
            .return_once(|| Box::pin(async { Ok(status) }));
        let poll_period = Duration::ZERO;
        let sequencing_monitor = TaikoSequencingMonitor::new(header, poll_period, status_monitor);

        assert!(
            tokio::time::timeout(TEST_DURATION, sequencing_monitor.ready())
                .await
                .is_ok()
        );
    }
}
