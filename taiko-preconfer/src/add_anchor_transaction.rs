use std::time::{Duration, SystemTime, UNIX_EPOCH};

use alloy_consensus::{SignableTransaction, TxEip1559, TxEnvelope};
use alloy_primitives::{Address, Bytes};
use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;
use block_building::taiko::hekla::CHAIN_ID;

use crate::error::PreconferResult;

pub fn get_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
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
