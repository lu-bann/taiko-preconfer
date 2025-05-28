use std::time::{Duration, SystemTime, UNIX_EPOCH};

use alloy_consensus::{Header, SignableTransaction, TxEip1559, TxEnvelope};
use alloy_primitives::{Address, Bytes, ChainId, FixedBytes};
use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;
use block_building::taiko::{
    contracts::TaikoAnchor,
    create_anchor_transaction,
    hekla::{CHAIN_ID, addresses::get_taiko_anchor_address},
    sign::get_signed_with_golden_touch,
};

use crate::error::PreconferResult;

pub fn get_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
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
    let anchor_call = create_anchor_call(
        anchor_block_id,
        anchor_state_root,
        parent_gas_used,
        base_fee_config,
    );
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
pub fn get_anchor_id(current_block_number: u64, lag: u64) -> u64 {
    current_block_number - std::cmp::min(current_block_number, lag)
}

pub fn insert_anchor_transaction(
    txs: Vec<TxEnvelope>,
    parent_header: Header,
    anchor_l1_header: Header,
    base_fee: u128,
    base_fee_config: TaikoAnchor::BaseFeeConfig,
    nonce: u64,
) -> PreconferResult<Vec<TxEnvelope>> {
    let anchor_tx = create_signed_anchor_transaction(
        CHAIN_ID,
        anchor_l1_header.number,
        anchor_l1_header.state_root,
        parent_header.gas_used as u32,
        base_fee_config,
        nonce,
        base_fee,
        0u128,
    )?;
    let mut txs = txs;
    txs.insert(0, anchor_tx);
    Ok(txs)
}

pub fn get_signed_eip1559_tx(
    to: Address,
    input: Bytes,
    nonce: u64,
    gas_limit: u64,
    max_fee_per_gas: u128,
    max_priority_fee_per_gas: u128,
    signer: &PrivateKeySigner,
) -> PreconferResult<TxEnvelope> {
    let tx = TxEip1559 {
        chain_id: CHAIN_ID,
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
