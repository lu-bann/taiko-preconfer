use alloy_consensus::Header;
use std::sync::Arc;
use tokio::{join, sync::RwLock};
use tracing::{debug, info};

use alloy::primitives::Address;

use crate::preconf::BlockBuilderResult;
use crate::taiko::anchor::ValidAnchor;
use crate::taiko::taiko_l2_client::ITaikoL2Client;
use crate::time_provider::ITimeProvider;

#[derive(Debug)]
pub struct BlockBuilder<L2Client: ITaikoL2Client, TimeProvider: ITimeProvider> {
    valid_anchor_id: ValidAnchor,
    l2_client: L2Client,
    time_provider: TimeProvider,
    last_l2_header: Arc<RwLock<Header>>,
    golden_touch_address: Address,
}

impl<L2Client: ITaikoL2Client, TimeProvider: ITimeProvider> BlockBuilder<L2Client, TimeProvider> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        valid_anchor_id: ValidAnchor,
        l2_client: L2Client,
        time_provider: TimeProvider,
        last_l2_header: Arc<RwLock<Header>>,
        golden_touch_address: Address,
    ) -> Self {
        Self {
            valid_anchor_id,
            l2_client,
            time_provider,
            last_l2_header,
            golden_touch_address,
        }
    }

    pub async fn build_block(
        &self,
        slot_timestamp: u64,
        end_of_sequencing: bool,
    ) -> BlockBuilderResult<()> {
        let last_l2_header = self.last_l2_header.read().await.clone();

        info!("Start preconfirming block: #{}", last_l2_header.number + 1,);
        let now = self.time_provider.timestamp_in_s();
        debug!(
            "slot={}, now={} parent={}",
            slot_timestamp, now, last_l2_header.timestamp
        );

        let (anchor_block_id, anchor_state_root) = self.valid_anchor_id.id_and_state_root().await;
        let (golden_touch_nonce, base_fee) = join!(
            self.l2_client.get_nonce(self.golden_touch_address),
            self.l2_client
                .get_base_fee(last_l2_header.gas_used, slot_timestamp),
        );

        let base_fee: u128 = base_fee?;
        debug!("base fee={base_fee}, anchor id={anchor_block_id}");
        let mut txs = self.l2_client.get_mempool_txs(base_fee as u64).await?;
        info!(
            "{} Found {} transactions in the mempool.",
            if txs.is_empty() { "∅" } else { "👀" },
            txs.len()
        );
        debug!("{:?}", txs);
        if txs.is_empty() && !end_of_sequencing {
            return Ok(());
        }

        let anchor_tx = self.l2_client.get_signed_anchor_tx(
            anchor_block_id,
            anchor_state_root,
            last_l2_header.gas_used as u32,
            golden_touch_nonce?,
            base_fee,
        )?;
        txs.insert(0, anchor_tx);

        info!(
            "📢 Publish preconfirmed block with {} transactions",
            txs.len()
        );
        let header = self
            .l2_client
            .publish_preconfirmed_transactions(
                base_fee as u64,
                slot_timestamp,
                &last_l2_header,
                txs.clone(),
                end_of_sequencing,
            )
            .await?;
        debug!("Received header: {header:?}");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::{U256, address};
    use alloy_consensus::{TxEip1559, TxEnvelope};

    use alloy::signers::Signature;
    use std::time::{Duration, UNIX_EPOCH};

    use crate::{
        taiko::taiko_l2_client::MockITaikoL2Client,
        test_util::{get_header, get_rpc_header},
        time_provider::MockITimeProvider,
    };

    use super::*;

    const DUMMY_NONCE: u64 = 10;
    const DUMMY_BASE_FEE: u128 = 100;
    const DUMMY_BLOCK_NUMBER: u64 = 1234;
    const DUMMY_TIMESTAMP: u64 = 987654321;
    const DUMMY_GAS: u64 = 30000;

    fn test_valid_anchor_id() -> ValidAnchor {
        let max_offset = 10;
        let desired_offset = 4;
        ValidAnchor::new(max_offset, desired_offset, "url".into())
    }

    #[tokio::test]
    async fn build_blocks_adds_anchor_transaction() {
        let l2_block_time_secs = 2000_u64;
        let last_block_timestamp = 1_000_000u64;
        let next_block_desired_timestamp = last_block_timestamp + l2_block_time_secs;

        let mut l2_client = MockITaikoL2Client::new();
        l2_client
            .expect_get_nonce()
            .return_once(|_| Box::pin(async { Ok(DUMMY_NONCE) }));
        l2_client
            .expect_get_base_fee()
            .return_once(|_, _| Box::pin(async { Ok(DUMMY_BASE_FEE) }));
        l2_client
            .expect_get_signed_anchor_tx()
            .return_once(|_, _, _, _, _| {
                Ok(TxEnvelope::new_unhashed(
                    TxEip1559::default().into(),
                    Signature::new(U256::ONE, U256::default(), false),
                ))
            });
        l2_client
            .expect_estimate_gas()
            .return_once(|_| Box::pin(async { Ok(DUMMY_GAS) }));
        l2_client
            .expect_publish_preconfirmed_transactions()
            .withf(|_, _, _, txs, _| txs.len() == 2 && txs[0].signature().r() == U256::ONE)
            .return_once(|_, _, _, _, _| {
                Box::pin(async {
                    Ok(get_rpc_header(get_header(
                        DUMMY_BLOCK_NUMBER,
                        DUMMY_TIMESTAMP,
                    )))
                })
            });
        l2_client.expect_get_mempool_txs().return_once(|_| {
            Box::pin(async {
                Ok(vec![TxEnvelope::new_unhashed(
                    TxEip1559::default().into(),
                    Signature::new(U256::default(), U256::default(), false),
                )])
            })
        });

        let mut time_provider = MockITimeProvider::new();
        time_provider.expect_now().return_const(
            UNIX_EPOCH
                .checked_add(Duration::from_secs(next_block_desired_timestamp))
                .unwrap(),
        );
        time_provider
            .expect_timestamp_in_s()
            .return_const(next_block_desired_timestamp);

        let valid_anchor_id = test_valid_anchor_id();
        let last_l2_header = Arc::new(RwLock::new(get_header(
            DUMMY_BLOCK_NUMBER,
            last_block_timestamp,
        )));
        let golden_touch_addr = address!("0x0000777735367b36bC9B61C50022d9D0700dB4Ec");
        let preconfer = BlockBuilder::new(
            valid_anchor_id,
            l2_client,
            time_provider,
            last_l2_header,
            golden_touch_addr,
        );

        let slot_timestamp = 135;
        assert!(preconfer.build_block(slot_timestamp, true).await.is_ok());
    }

    #[tokio::test]
    async fn no_block_gets_published_when_mempool_is_empty() {
        let l2_block_time_secs = 2000_u64;
        let last_block_timestamp = 1_000_000u64;
        let next_block_desired_timestamp = last_block_timestamp + l2_block_time_secs;

        let mut l2_client = MockITaikoL2Client::new();
        l2_client
            .expect_get_nonce()
            .return_once(|_| Box::pin(async { Ok(DUMMY_NONCE) }));
        l2_client
            .expect_get_base_fee()
            .return_once(|_, _| Box::pin(async { Ok(DUMMY_BASE_FEE) }));
        l2_client.expect_get_signed_anchor_tx().never();
        l2_client.expect_publish_preconfirmed_transactions().never();
        l2_client
            .expect_get_mempool_txs()
            .return_once(|_| Box::pin(async { Ok(vec![]) }));

        let mut time_provider = MockITimeProvider::new();
        time_provider.expect_now().return_const(
            UNIX_EPOCH
                .checked_add(Duration::from_secs(next_block_desired_timestamp))
                .unwrap(),
        );
        time_provider
            .expect_timestamp_in_s()
            .return_const(next_block_desired_timestamp);

        let valid_anchor_id = test_valid_anchor_id();
        let last_l2_header = Arc::new(RwLock::new(get_header(
            DUMMY_BLOCK_NUMBER,
            last_block_timestamp,
        )));
        let golden_touch_addr = address!("0x0000777735367b36bC9B61C50022d9D0700dB4Ec");
        let preconfer = BlockBuilder::new(
            valid_anchor_id,
            l2_client,
            time_provider,
            last_l2_header,
            golden_touch_addr,
        );

        let slot_timestamp = 135;
        assert!(preconfer.build_block(slot_timestamp, false).await.is_ok());
    }
}
