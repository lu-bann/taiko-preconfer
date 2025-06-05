use alloy_consensus::{Header, TxEnvelope};
use alloy_rpc_types::Header as RpcHeader;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::{join, sync::Mutex};
use tracing::{debug, info};

use alloy_primitives::Address;

use crate::preconf::{PreconferError, PreconferResult};
use crate::taiko::taiko_client::ITaikoClient;
use crate::taiko::taiko_l1_client::ITaikoL1Client;
use crate::time_provider::ITimeProvider;

#[derive(Debug)]
pub struct SimpleBlock {
    pub header: RpcHeader,
    pub txs: Vec<TxEnvelope>,
    pub anchor_block_id: u64,
    pub last_block_timestamp: u64,
}

impl SimpleBlock {
    pub const fn new(
        header: RpcHeader,
        txs: Vec<TxEnvelope>,
        anchor_block_id: u64,
        last_block_timestamp: u64,
    ) -> Self {
        Self {
            header,
            txs,
            anchor_block_id,
            last_block_timestamp,
        }
    }
}

#[derive(Debug)]
pub struct Preconfer<L1Client: ITaikoL1Client, Taiko: ITaikoClient, TimeProvider: ITimeProvider> {
    anchor_id_lag: u64,
    l1_client: L1Client,
    taiko: Taiko,
    address: Address,
    time_provider: TimeProvider,
    last_l1_block_number: Arc<Mutex<u64>>,
    parent_header: Arc<Mutex<Option<Header>>>,
    golden_touch_address: String,
}

impl<L1Client: ITaikoL1Client, Taiko: ITaikoClient, TimeProvider: ITimeProvider>
    Preconfer<L1Client, Taiko, TimeProvider>
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        anchor_id_lag: u64,
        l1_client: L1Client,
        taiko: Taiko,
        address: Address,
        time_provider: TimeProvider,
        last_l1_block_number: Arc<Mutex<u64>>,
        parent_header: Arc<Mutex<Option<Header>>>,
        golden_touch_address: String,
    ) -> Self {
        Self {
            anchor_id_lag,
            l1_client,
            taiko,
            address,
            time_provider,
            last_l1_block_number,
            parent_header,
            golden_touch_address,
        }
    }

    pub fn address(&self) -> Address {
        self.address
    }

    fn last_l1_block_number(&self) -> PreconferResult<u64> {
        Ok(self.last_l1_block_number.try_lock().map(|guard| *guard)?)
    }

    pub async fn build_block(&self) -> PreconferResult<Option<SimpleBlock>> {
        let parent_header = self.parent_header.lock().await.clone();
        if parent_header.is_none() {
            return Err(PreconferError::MissingParentHeader);
        }
        let parent_header = parent_header.unwrap();

        let start = SystemTime::now();

        info!("Start preconfirming block: #{}", parent_header.number + 1,);
        let now = self.time_provider.timestamp_in_s();
        debug!("now={} parent={}", now, parent_header.timestamp);

        let last_l1_block_number = self.last_l1_block_number()?;
        let anchor_block_id = get_anchor_id(last_l1_block_number, self.anchor_id_lag);
        let (anchor_header, golden_touch_nonce, base_fee) = join!(
            self.l1_client.get_header(anchor_block_id),
            self.taiko.get_nonce(&self.golden_touch_address),
            self.taiko.get_base_fee(parent_header.gas_used, now),
        );

        let base_fee: u128 = base_fee?;
        let mut txs = self
            .taiko
            .get_mempool_txs(self.address, base_fee as u64)
            .await?;
        debug!("#txs in mempool: {}", txs.len());
        if txs.is_empty() {
            info!("Empty mempool: skipping block building.");
            return Ok(None);
        }

        let anchor_tx = self.taiko.get_signed_anchor_tx(
            anchor_block_id,
            anchor_header?.state_root,
            parent_header.gas_used as u32,
            golden_touch_nonce?,
            base_fee,
        )?;
        txs.insert(0, anchor_tx);

        info!("Publish preconfirmed block with {} transactions", txs.len());
        let header = self
            .taiko
            .publish_preconfirmed_transactions(
                self.address,
                base_fee as u64,
                now, // after await, recompute?
                &parent_header,
                txs.clone(),
            )
            .await?;

        let end = SystemTime::now();
        debug!(
            "elapsed: {} ms",
            end.duration_since(start).unwrap().as_millis()
        );
        Ok(Some(SimpleBlock::new(
            header,
            txs,
            anchor_block_id,
            parent_header.timestamp,
        )))
    }
}

fn get_anchor_id(current_block_number: u64, lag: u64) -> u64 {
    current_block_number - std::cmp::min(current_block_number, lag)
}

#[cfg(test)]
mod tests {
    use alloy_consensus::TxEip1559;
    use alloy_primitives::U256;
    use alloy_provider::utils::Eip1559Estimation;
    use alloy_signer::Signature;
    use std::time::{Duration, UNIX_EPOCH};

    use crate::{
        taiko::{taiko_client::MockITaikoClient, taiko_l1_client::MockITaikoL1Client},
        test_util::{get_header, get_rpc_header},
        time_provider::MockITimeProvider,
    };

    use super::*;

    const DUMMY_NONCE: u64 = 10;
    const DUMMY_BASE_FEE: u128 = 100;
    const DUMMY_BLOCK_NUMBER: u64 = 1234;
    const DUMMY_TIMESTAMP: u64 = 987654321;
    const DUMMY_GAS: u64 = 30000;
    const DUMMY_MAX_FEE_PER_GAS: u128 = 50000;
    const DUMMY_MAX_PRIORITY_FEE_PER_GAS: u128 = 70000;

    #[tokio::test]
    async fn test_build_blocks_adds_anchor_transaction() {
        let l2_block_time_secs = 2000_u64;
        let last_block_timestamp = 1_000_000u64;
        let next_block_desired_timestamp = last_block_timestamp + l2_block_time_secs;

        let mut l1_client = MockITaikoL1Client::new();
        l1_client
            .expect_get_nonce()
            .return_once(|_| Box::pin(async { Ok(DUMMY_NONCE) }));
        l1_client
            .expect_get_header()
            .return_once(|_| Box::pin(async { Ok(Header::default()) }));
        l1_client.expect_estimate_eip1559_fees().return_once(|| {
            Box::pin(async {
                Ok(Eip1559Estimation {
                    max_fee_per_gas: DUMMY_MAX_FEE_PER_GAS,
                    max_priority_fee_per_gas: DUMMY_MAX_PRIORITY_FEE_PER_GAS,
                })
            })
        });

        let mut taiko = MockITaikoClient::new();
        taiko
            .expect_get_nonce()
            .return_once(|_| Box::pin(async { Ok(DUMMY_NONCE) }));
        taiko
            .expect_get_base_fee()
            .return_once(|_, _| Box::pin(async { Ok(DUMMY_BASE_FEE) }));
        taiko
            .expect_get_signed_anchor_tx()
            .return_once(|_, _, _, _, _| {
                Ok(TxEnvelope::new_unhashed(
                    TxEip1559::default().into(),
                    Signature::new(U256::ONE, U256::default(), false),
                ))
            });
        taiko
            .expect_estimate_gas()
            .return_once(|_| Box::pin(async { Ok(DUMMY_GAS) }));
        taiko
            .expect_publish_preconfirmed_transactions()
            .withf(|_, _, _, _, txs| txs.len() == 2 && txs[0].signature().r() == U256::ONE)
            .return_once(|_, _, _, _, _| {
                Box::pin(async {
                    Ok(get_rpc_header(get_header(
                        DUMMY_BLOCK_NUMBER,
                        DUMMY_TIMESTAMP,
                    )))
                })
            });
        taiko.expect_get_mempool_txs().return_once(|_, _| {
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

        let preconfer_address = Address::random();

        let anchor_id_lag = 4u64;
        let parent_header = Arc::new(Mutex::new(Some(get_header(
            DUMMY_BLOCK_NUMBER,
            last_block_timestamp,
        ))));
        let last_l1_block_number = Arc::new(Mutex::new(DUMMY_BLOCK_NUMBER));
        let preconfer = Preconfer::new(
            anchor_id_lag,
            l1_client,
            taiko,
            preconfer_address,
            time_provider,
            last_l1_block_number,
            parent_header,
            "0x0000777735367b36bC9B61C50022d9D0700dB4Ec".into(),
        );

        let preconfirmed_block = preconfer.build_block().await.unwrap().unwrap();
        assert_eq!(preconfirmed_block.txs.len(), 2);
        assert_eq!(preconfirmed_block.txs[0].signature().r(), U256::ONE);
    }

    #[tokio::test]
    async fn test_no_block_gets_published_when_mempool_is_empty() {
        let l2_block_time_secs = 2000_u64;
        let last_block_timestamp = 1_000_000u64;
        let next_block_desired_timestamp = last_block_timestamp + l2_block_time_secs;

        let mut l1_client = MockITaikoL1Client::new();
        l1_client
            .expect_get_header()
            .return_once(|_| Box::pin(async { Ok(Header::default()) }));

        let mut taiko = MockITaikoClient::new();
        taiko
            .expect_get_nonce()
            .return_once(|_| Box::pin(async { Ok(DUMMY_NONCE) }));
        taiko
            .expect_get_base_fee()
            .return_once(|_, _| Box::pin(async { Ok(DUMMY_BASE_FEE) }));
        taiko.expect_get_signed_anchor_tx().never();
        taiko.expect_publish_preconfirmed_transactions().never();
        taiko
            .expect_get_mempool_txs()
            .return_once(|_, _| Box::pin(async { Ok(vec![]) }));

        let mut time_provider = MockITimeProvider::new();
        time_provider.expect_now().return_const(
            UNIX_EPOCH
                .checked_add(Duration::from_secs(next_block_desired_timestamp))
                .unwrap(),
        );
        time_provider
            .expect_timestamp_in_s()
            .return_const(next_block_desired_timestamp);

        let preconfer_address = Address::random();

        let anchor_id_lag = 4u64;
        let parent_header = Arc::new(Mutex::new(Some(get_header(
            DUMMY_BLOCK_NUMBER,
            last_block_timestamp,
        ))));
        let last_l1_block_number = Arc::new(Mutex::new(DUMMY_BLOCK_NUMBER));
        let preconfer = Preconfer::new(
            anchor_id_lag,
            l1_client,
            taiko,
            preconfer_address,
            time_provider,
            last_l1_block_number,
            parent_header,
            "0x0000777735367b36bC9B61C50022d9D0700dB4Ec".into(),
        );

        assert!(preconfer.build_block().await.unwrap().is_none());
    }
}
