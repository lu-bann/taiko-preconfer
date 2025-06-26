use alloy_consensus::{Header, TxEnvelope};
use alloy_rpc_types::Header as RpcHeader;
use std::sync::Arc;
use tokio::{join, sync::RwLock};
use tracing::{debug, info, trace};

use alloy_primitives::Address;

use crate::preconf::BlockBuilderResult;
use crate::taiko::taiko_l1_client::ITaikoL1Client;
use crate::taiko::taiko_l2_client::ITaikoL2Client;
use crate::time_provider::ITimeProvider;

#[derive(Debug, Clone)]
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
pub struct BlockBuilder<
    L1Client: ITaikoL1Client,
    L2Client: ITaikoL2Client,
    TimeProvider: ITimeProvider,
> {
    anchor_id_lag: u64,
    l1_client: L1Client,
    l2_client: L2Client,
    address: Address,
    time_provider: TimeProvider,
    last_l1_header: Arc<RwLock<Header>>,
    last_l2_header: Arc<RwLock<Header>>,
    golden_touch_address: String,
}

impl<L1Client: ITaikoL1Client, L2Client: ITaikoL2Client, TimeProvider: ITimeProvider>
    BlockBuilder<L1Client, L2Client, TimeProvider>
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        anchor_id_lag: u64,
        l1_client: L1Client,
        l2_client: L2Client,
        address: Address,
        time_provider: TimeProvider,
        last_l1_header: Arc<RwLock<Header>>,
        last_l2_header: Arc<RwLock<Header>>,
        golden_touch_address: String,
    ) -> Self {
        Self {
            anchor_id_lag,
            l1_client,
            l2_client,
            address,
            time_provider,
            last_l1_header,
            last_l2_header,
            golden_touch_address,
        }
    }

    pub fn address(&self) -> Address {
        self.address
    }

    pub async fn build_block(&self) -> BlockBuilderResult<Option<SimpleBlock>> {
        let last_l2_header = self.last_l2_header.read().await.clone();
        trace!("build_block: last_l2_header={last_l2_header:?}");

        info!("Start preconfirming block: #{}", last_l2_header.number + 1,);
        let now = self.time_provider.timestamp_in_s();
        debug!("now={} parent={}", now, last_l2_header.timestamp);

        let last_l1_header = self.last_l1_header.read().await.clone();
        let anchor_block_id = get_anchor_id(last_l1_header.number, self.anchor_id_lag);
        let (anchor_header, golden_touch_nonce, base_fee) = join!(
            self.l1_client.get_header(anchor_block_id),
            self.l2_client.get_nonce(&self.golden_touch_address),
            self.l2_client.get_base_fee(last_l2_header.gas_used, now),
        );

        let base_fee: u128 = base_fee?;
        trace!("base fee: {base_fee}");
        let mut txs = self
            .l2_client
            .get_mempool_txs(self.address, base_fee as u64)
            .await?;
        debug!("#txs in mempool: {}", txs.len());
        trace!("{:?}", txs);
        if txs.is_empty() {
            info!("Empty mempool: skipping block building.");
            return Ok(None);
        }

        let anchor_tx = self.l2_client.get_signed_anchor_tx(
            anchor_block_id,
            anchor_header?.state_root,
            last_l2_header.gas_used as u32,
            golden_touch_nonce?,
            base_fee,
        )?;
        txs.insert(0, anchor_tx);

        info!("Publish preconfirmed block with {} transactions", txs.len());
        let header = self
            .l2_client
            .publish_preconfirmed_transactions(
                self.address,
                base_fee as u64,
                now, // after await, recompute?
                &last_l2_header,
                txs.clone(),
            )
            .await?;
        trace!("Preconfirmed block header: {header:?}");

        Ok(Some(SimpleBlock::new(
            header,
            txs,
            anchor_block_id,
            last_l1_header.timestamp,
        )))
    }
}

pub fn get_anchor_id(current_block_number: u64, lag: u64) -> u64 {
    current_block_number - std::cmp::min(current_block_number, lag)
}

#[cfg(test)]
mod tests {
    use alloy_consensus::TxEip1559;
    use alloy_primitives::U256;

    use alloy_signer::Signature;
    use std::time::{Duration, UNIX_EPOCH};

    use crate::{
        taiko::{taiko_l1_client::MockITaikoL1Client, taiko_l2_client::MockITaikoL2Client},
        test_util::{get_header, get_rpc_header},
        time_provider::MockITimeProvider,
    };

    use super::*;

    const DUMMY_NONCE: u64 = 10;
    const DUMMY_BASE_FEE: u128 = 100;
    const DUMMY_BLOCK_NUMBER: u64 = 1234;
    const DUMMY_TIMESTAMP: u64 = 987654321;
    const DUMMY_GAS: u64 = 30000;

    #[tokio::test]
    async fn build_blocks_adds_anchor_transaction() {
        let l2_block_time_secs = 2000_u64;
        let last_block_timestamp = 1_000_000u64;
        let next_block_desired_timestamp = last_block_timestamp + l2_block_time_secs;

        let mut l1_client = MockITaikoL1Client::new();
        l1_client
            .expect_get_header()
            .return_once(|_| Box::pin(async { Ok(Header::default()) }));

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
            .withf(|_, _, _, _, txs| txs.len() == 2 && txs[0].signature().r() == U256::ONE)
            .return_once(|_, _, _, _, _| {
                Box::pin(async {
                    Ok(get_rpc_header(get_header(
                        DUMMY_BLOCK_NUMBER,
                        DUMMY_TIMESTAMP,
                    )))
                })
            });
        l2_client.expect_get_mempool_txs().return_once(|_, _| {
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
        let last_l2_header = Arc::new(RwLock::new(get_header(
            DUMMY_BLOCK_NUMBER,
            last_block_timestamp,
        )));
        let last_l1_header = Arc::new(RwLock::new(get_header(
            DUMMY_BLOCK_NUMBER,
            last_block_timestamp,
        )));
        let golden_touch_addr = String::from("0x0000777735367b36bC9B61C50022d9D0700dB4Ec");
        let preconfer = BlockBuilder::new(
            anchor_id_lag,
            l1_client,
            l2_client,
            preconfer_address,
            time_provider,
            last_l1_header,
            last_l2_header,
            golden_touch_addr,
        );

        let preconfirmed_block = preconfer.build_block().await.unwrap().unwrap();
        assert_eq!(preconfirmed_block.txs.len(), 2);
        assert_eq!(preconfirmed_block.txs[0].signature().r(), U256::ONE);
    }

    #[tokio::test]
    async fn no_block_gets_published_when_mempool_is_empty() {
        let l2_block_time_secs = 2000_u64;
        let last_block_timestamp = 1_000_000u64;
        let next_block_desired_timestamp = last_block_timestamp + l2_block_time_secs;

        let mut l1_client = MockITaikoL1Client::new();
        l1_client
            .expect_get_header()
            .return_once(|_| Box::pin(async { Ok(Header::default()) }));

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
        let last_l2_header = Arc::new(RwLock::new(get_header(
            DUMMY_BLOCK_NUMBER,
            last_block_timestamp,
        )));
        let last_l1_header = Arc::new(RwLock::new(get_header(
            DUMMY_BLOCK_NUMBER,
            last_block_timestamp,
        )));
        let golden_touch_addr = String::from("0x0000777735367b36bC9B61C50022d9D0700dB4Ec");
        let preconfer = BlockBuilder::new(
            anchor_id_lag,
            l1_client,
            l2_client,
            preconfer_address,
            time_provider,
            last_l1_header,
            last_l2_header,
            golden_touch_addr,
        );

        assert!(preconfer.build_block().await.unwrap().is_none());
    }
}
