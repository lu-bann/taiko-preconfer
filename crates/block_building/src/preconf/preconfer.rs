use alloy_consensus::{SignableTransaction, TxEip1559, TxEnvelope};
use alloy_provider::network::TransactionBuilder;
use alloy_rpc_types::{Header, TransactionRequest};
use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::{join, sync::Mutex, time::sleep};
use tracing::{debug, info};

use alloy_primitives::{Address, B256, Bytes, ChainId};

use crate::compression::compress;
use crate::preconf::PreconferResult;
use crate::taiko::contracts::taiko_wrapper::BlockParams;
use crate::taiko::hekla::addresses::{GOLDEN_TOUCH_ADDRESS, get_taiko_inbox_address};
use crate::taiko::propose_batch::create_propose_batch_params;
use crate::taiko::taiko_client::ITaikoClient;
use crate::taiko::taiko_l1_client::ITaikoL1Client;
use crate::time_provider::ITimeProvider;

#[derive(Debug)]
pub struct Preconfer<L1Client: ITaikoL1Client, Taiko: ITaikoClient, TimeProvider: ITimeProvider> {
    last_l1_block_number: Arc<Mutex<u64>>,
    config: Config,
    l1_client: L1Client,
    taiko: Taiko,
    address: Address,
    time_provider: TimeProvider,
}

impl<L1Client: ITaikoL1Client, Taiko: ITaikoClient, TimeProvider: ITimeProvider>
    Preconfer<L1Client, Taiko, TimeProvider>
{
    pub fn new(
        config: Config,
        l1_client: L1Client,
        taiko: Taiko,
        address: Address,
        time_provider: TimeProvider,
    ) -> Self {
        Self {
            last_l1_block_number: Arc::new(Mutex::new(0u64)),
            config,
            l1_client,
            taiko,
            address,
            time_provider,
        }
    }

    pub fn shared_last_l1_block_number(&self) -> Arc<Mutex<u64>> {
        self.last_l1_block_number.clone()
    }

    pub fn last_l1_block_number(&self) -> PreconferResult<u64> {
        Ok(self.last_l1_block_number.try_lock().map(|guard| *guard)?)
    }

    async fn wait_until_next_block(&self, current_block_timestamp: u64) -> PreconferResult<()> {
        let now = self.time_provider.now();
        let desired_next_block_time = UNIX_EPOCH
            .checked_add(Duration::from_secs(current_block_timestamp))
            .map(|x| {
                x.checked_add(self.config.l2_block_time)
                    .unwrap_or(UNIX_EPOCH)
            })
            .unwrap_or(UNIX_EPOCH);
        let remaining = desired_next_block_time.duration_since(now)?;
        sleep(remaining).await;
        Ok(())
    }

    pub async fn build_block(&self, parent_header: Header) -> PreconferResult<()> {
        self.wait_until_next_block(parent_header.timestamp).await?;

        let start = SystemTime::now();

        info!("Start preconfirming block: #{}", parent_header.number + 1,);
        let now = self.time_provider.timestamp_in_s();
        debug!("t: now={} parent={}", now, parent_header.timestamp);

        let last_l1_block_number = self.last_l1_block_number()?;
        let anchor_block_id = get_anchor_id(last_l1_block_number, self.config.anchor_id_lag);
        debug!("t: get anchor header with id {}", anchor_block_id);
        let (anchor_header, golden_touch_nonce, base_fee) = join!(
            self.l1_client.get_header(anchor_block_id),
            self.taiko.get_nonce(GOLDEN_TOUCH_ADDRESS),
            self.taiko.get_base_fee(parent_header.gas_used, now),
        );

        let base_fee: u128 = base_fee?;
        let mut txs = self
            .taiko
            .get_mempool_txs(self.address, base_fee as u64)
            .await?;
        debug!("#txs in mempool: {}", txs.len());

        let anchor_tx = self.taiko.get_signed_anchor_tx(
            anchor_block_id,
            anchor_header?.state_root,
            parent_header.gas_used as u32,
            golden_touch_nonce?,
            base_fee,
        )?;
        txs.insert(0, anchor_tx);

        let _header = self
            .taiko
            .publish_preconfirmed_transactions(
                self.address,
                base_fee as u64,
                now, // after await, recompute?
                &parent_header,
                txs.clone(),
            )
            .await?;

        let number_of_blobs = 0u8;
        let parent_meta_hash = B256::ZERO;
        let coinbase = parent_header.beneficiary;
        let tx_bytes = Bytes::from(compress(txs.clone())?);
        let block_params = vec![BlockParams {
            numTransactions: txs.len() as u16,
            timeShift: 0,
            signalSlots: vec![],
        }];
        let propose_batch_params = create_propose_batch_params(
            self.address,
            tx_bytes,
            block_params,
            parent_meta_hash,
            anchor_block_id,
            parent_header.timestamp,
            coinbase,
            number_of_blobs,
        );

        let signer = PrivateKeySigner::random();
        let taiko_inbox_address = get_taiko_inbox_address();
        let tx = TransactionRequest::default()
            .with_to(taiko_inbox_address)
            .with_input(propose_batch_params.clone())
            .with_from(signer.address());
        let signer_str = signer.address().to_string();
        let (nonce, gas_limit, fee_estimate) = join!(
            self.l1_client.get_nonce(&signer_str),
            self.l1_client.estimate_gas(tx),
            self.l1_client.estimate_eip1559_fees(),
        );
        let fee_estimate = fee_estimate?;

        let signed_tx = get_signed_eip1559_tx(
            0, // TODO: Add L1 Chain id
            taiko_inbox_address,
            propose_batch_params,
            nonce?,
            gas_limit?,
            fee_estimate.max_fee_per_gas,
            fee_estimate.max_priority_fee_per_gas,
            &signer,
        )?;

        debug!("signed propose batch tx {signed_tx:?}");
        let end = SystemTime::now();
        debug!(
            "elapsed: {} ms",
            end.duration_since(start).unwrap().as_millis()
        );
        Ok(())
    }
}

#[derive(Debug)]
pub struct Config {
    pub l2_block_time: Duration,
    #[allow(dead_code)]
    pub handover_window_slots: u32,
    #[allow(dead_code)]
    pub handover_start_buffer: Duration,
    pub anchor_id_lag: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            l2_block_time: Duration::from_millis(12000),
            handover_window_slots: 4,
            handover_start_buffer: Duration::from_millis(6000),
            anchor_id_lag: 4,
        }
    }
}
fn get_anchor_id(current_block_number: u64, lag: u64) -> u64 {
    current_block_number - std::cmp::min(current_block_number, lag)
}

#[allow(clippy::too_many_arguments)]
fn get_signed_eip1559_tx(
    chain_id: ChainId,
    to: Address,
    input: Bytes,
    nonce: u64,
    gas_limit: u64,
    max_fee_per_gas: u128,
    max_priority_fee_per_gas: u128,
    signer: &PrivateKeySigner,
) -> PreconferResult<TxEnvelope> {
    let tx = TxEip1559 {
        chain_id,
        nonce,
        gas_limit,
        max_fee_per_gas,
        max_priority_fee_per_gas,
        to: to.into(),
        input,
        value: Default::default(),
        access_list: Default::default(),
    };

    let sig = signer.sign_hash_sync(&tx.signature_hash())?;
    let signed = tx.into_signed(sig);

    Ok(signed.into())
}

#[cfg(test)]
mod tests {
    use alloy_consensus::Header as ConsensusHeader;
    use alloy_primitives::{FixedBytes, U256};
    use alloy_provider::utils::Eip1559Estimation;
    use alloy_signer::Signature;

    use crate::{
        taiko::{taiko_client::MockITaikoClient, taiko_l1_client::MockITaikoL1Client},
        time_provider::MockITimeProvider,
    };

    use super::*;

    fn get_rpc_header(inner: ConsensusHeader) -> Header {
        Header {
            hash: FixedBytes::<32>::default(),
            inner,
            total_difficulty: None,
            size: None,
        }
    }

    fn get_header(number: u64, timestamp: u64) -> ConsensusHeader {
        ConsensusHeader {
            number,
            timestamp,
            ..Default::default()
        }
    }

    const DUMMY_NONCE: u64 = 10;
    const DUMMY_BASE_FEE: u128 = 100;
    const DUMMY_BLOCK_NUMBER: u64 = 1234;
    const DUMMY_TIMESTAMP: u64 = 987654321;
    const DUMMY_GAS: u64 = 30000;
    const DUMMY_MAX_FEE_PER_GAS: u128 = 50000;
    const DUMMY_MAX_PRIORITY_FEE_PER_GAS: u128 = 70000;

    #[tokio::test]
    async fn test_build_blocks_adds_anchor_transaction() {
        let config = Config::default();
        let last_block_timestamp = 1_000_000u64;
        let next_block_desired_timestamp = last_block_timestamp + config.l2_block_time.as_secs();

        let mut l1_client = MockITaikoL1Client::new();
        l1_client
            .expect_get_nonce()
            .return_once(|_| Box::pin(async { Ok(DUMMY_NONCE) }));
        l1_client
            .expect_get_header()
            .return_once(|_| Box::pin(async { Ok(ConsensusHeader::default()) }));
        l1_client
            .expect_estimate_gas()
            .return_once(|_| Box::pin(async { Ok(DUMMY_GAS) }));
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

        let preconfer = Preconfer::new(config, l1_client, taiko, preconfer_address, time_provider);
        *preconfer.shared_last_l1_block_number().lock().await = DUMMY_BLOCK_NUMBER;

        let parent_header = get_rpc_header(get_header(DUMMY_BLOCK_NUMBER, last_block_timestamp));
        assert!(preconfer.build_block(parent_header).await.is_ok());
    }
}
