use std::time::{SystemTime, UNIX_EPOCH};

use alloy_consensus::{Header, TxEnvelope};
use alloy_primitives::{ChainId, FixedBytes};
use block_building::{
    http_client::{HttpClient, get_header_by_id, get_nonce},
    taiko::{
        contracts::{TaikoAnchor, TaikoAnchorInstance},
        create_anchor_transaction,
        hekla::{
            CHAIN_ID,
            addresses::{GOLDEN_TOUCH_ADDRESS, get_taiko_anchor_address},
            get_basefee_config_v2,
        },
        sign::get_signed_with_golden_touch,
    },
};

use crate::error::PreconferResult;

fn get_timestamp() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

fn create_anchor_call(
    anchor_block_id: u64,
    anchor_state_root: FixedBytes<32>,
    parent_gas_used: u32,
    base_fee_config: TaikoAnchor::BaseFeeConfig,
) -> TaikoAnchor::anchorV3Call {
    TaikoAnchor::anchorV3Call {
        _anchorBlockId: anchor_block_id,
        _anchorStateRoot: anchor_state_root,
        _parentGasUsed: parent_gas_used,
        _baseFeeConfig: base_fee_config,
        _signalSlots: vec![],
    }
}

#[allow(clippy::too_many_arguments)]
pub fn create_signed_anchor_transaction(
    chain_id: ChainId,
    anchor_block_id: u64,
    anchor_state_root: FixedBytes<32>,
    parent_gas_used: u32,
    base_fee_config: TaikoAnchor::BaseFeeConfig,
    nonce: u64,
    max_fee_per_gas: u128,
    max_priority_fee_per_gas: u128,
) -> Result<TxEnvelope, k256::ecdsa::Error> {
    let anchor_call =
        create_anchor_call(anchor_block_id, anchor_state_root, parent_gas_used, base_fee_config);
    let anchor_tx = create_anchor_transaction(
        chain_id,
        nonce,
        max_fee_per_gas,
        max_priority_fee_per_gas,
        get_taiko_anchor_address(),
        anchor_call,
    );

    let signed_anchor_tx = get_signed_with_golden_touch(anchor_tx.clone())?;
    Ok(TxEnvelope::from(signed_anchor_tx))
}

pub struct Config {
    #[allow(dead_code)]
    pub l2_block_time_ms: u64,
    #[allow(dead_code)]
    pub handover_window_slots: u32,
    #[allow(dead_code)]
    pub handover_start_buffer_ms: u64,
    pub anchor_id_lag: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            l2_block_time_ms: 2000,
            handover_window_slots: 4,
            handover_start_buffer_ms: 6000,
            anchor_id_lag: 4,
        }
    }
}

pub struct BlockBuilder<Client: HttpClient> {
    l2_client: Client,
    l1_client: Client,
    anchor_id_lag: u64,
    chain_id: ChainId,
    base_fee_config: TaikoAnchor::BaseFeeConfig,
    taiko_anchor: TaikoAnchorInstance,
}

fn get_anchor_id(current_block_number: u64, lag: u64) -> u64 {
    current_block_number - std::cmp::min(current_block_number, lag)
}

impl<Client: HttpClient> BlockBuilder<Client> {
    pub fn new(
        l2_client: Client,
        l1_client: Client,
        anchor_id_lag: u64,
        taiko_anchor: TaikoAnchorInstance,
    ) -> Self {
        Self {
            l2_client,
            l1_client,
            anchor_id_lag,
            chain_id: CHAIN_ID,
            base_fee_config: get_basefee_config_v2(),
            taiko_anchor,
        }
    }

    pub async fn build_block(
        &self,
        txs: Vec<TxEnvelope>,
        parent_header: Header,
        current_l1_header: Header,
    ) -> PreconferResult<Vec<TxEnvelope>> {
        let anchor_block_id = get_anchor_id(current_l1_header.number, self.anchor_id_lag);
        let anchor_header = get_header_by_id(&self.l1_client, anchor_block_id).await?;
        let nonce = get_nonce(&self.l2_client, GOLDEN_TOUCH_ADDRESS).await?;
        let timestamp = get_timestamp();
        let base_fee: u128 = self
            .taiko_anchor
            .getBasefeeV2(parent_header.gas_used as u32, timestamp, self.base_fee_config.clone())
            .call()
            .await?
            .basefee_
            .try_into()?;

        let anchor_tx = create_signed_anchor_transaction(
            self.chain_id,
            anchor_block_id,
            anchor_header.state_root,
            parent_header.gas_used as u32,
            self.base_fee_config.clone(),
            nonce,
            base_fee,
            0u128,
        )?;
        let mut txs = txs;
        txs.insert(0, anchor_tx);
        Ok(txs)
    }
}
