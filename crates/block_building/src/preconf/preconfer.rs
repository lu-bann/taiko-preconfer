use alloy_consensus::{SignableTransaction, TxEip1559, TxEnvelope};
use alloy_provider::network::TransactionBuilder;
use alloy_rpc_types::{Header, TransactionRequest};
use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::{join, sync::Mutex, time::sleep};
use tracing::info;

use alloy_primitives::{Address, B256, Bytes, ChainId};

use crate::compression::compress;
use crate::http_client::{HttpClient, get_header_by_id, get_nonce};
use crate::preconf::PreconferResult;
use crate::taiko::contracts::taiko_wrapper::BlockParams;
use crate::taiko::hekla::addresses::{GOLDEN_TOUCH_ADDRESS, get_taiko_inbox_address};
use crate::taiko::propose_batch::create_propose_batch_params;
use crate::taiko::taiko_client::ITaikoClient;

#[derive(Debug)]
pub struct Preconfer<L1Client: HttpClient, Taiko: ITaikoClient> {
    last_l1_block_number: Arc<Mutex<u64>>,
    config: Config,
    l1_client: L1Client,
    taiko: Taiko,
    address: Address,
}

impl<L1Client: HttpClient, Taiko: ITaikoClient> Preconfer<L1Client, Taiko> {
    pub fn new(config: Config, l1_client: L1Client, taiko: Taiko, address: Address) -> Self {
        Self {
            last_l1_block_number: Arc::new(Mutex::new(0u64)),
            config,
            l1_client,
            taiko,
            address,
        }
    }

    pub fn shared_last_l1_block_number(&self) -> Arc<Mutex<u64>> {
        self.last_l1_block_number.clone()
    }

    pub fn last_l1_block_number(&self) -> PreconferResult<u64> {
        Ok(self.last_l1_block_number.try_lock().map(|guard| *guard)?)
    }

    async fn wait_until_next_block(&self, current_block_timestamp: u64) -> PreconferResult<()> {
        let now = SystemTime::now();
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

        let last_l1_block_number = self.last_l1_block_number()?;
        info!(
            "build block {}, l1 {}",
            parent_header.number + 1,
            last_l1_block_number
        );
        let now = get_timestamp();
        info!("t: now={} parent={}", now, parent_header.timestamp);

        let anchor_block_id = get_anchor_id(last_l1_block_number, self.config.anchor_id_lag);
        let (anchor_header, golden_touch_nonce, base_fee) = join!(
            get_header_by_id(&self.l1_client, anchor_block_id),
            self.taiko.get_nonce(GOLDEN_TOUCH_ADDRESS),
            self.taiko.get_base_fee(parent_header.gas_used, now),
        );

        let base_fee: u128 = base_fee?;
        let mut txs = self
            .taiko
            .get_mempool_txs(self.address, base_fee as u64)
            .await?;
        info!("#txs in mempool: {}", txs.len());

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
            get_nonce(&self.l1_client, &signer_str),
            self.taiko.estimate_gas(tx),
            self.taiko.estimate_eip1559_fees(),
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

        info!("signed propose batch tx {signed_tx:?}");
        let end = SystemTime::now();
        info!(
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

pub fn get_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
