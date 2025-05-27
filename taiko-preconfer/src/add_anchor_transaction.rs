use alloy_consensus::TxEnvelope;
use alloy_primitives::{ChainId, FixedBytes};
use block_building::taiko::{
    contracts::TaikoAnchor, create_anchor_transaction, hekla::addresses::get_taiko_anchor_address,
    sign::get_signed_with_golden_touch,
};

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

#[allow(clippy::too_many_arguments, dead_code)]
pub fn create_signed_anchor_transaction(
    chain_id: ChainId,
    anchor_block_id: u64,
    anchor_state_root: FixedBytes<32>,
    parent_gas_used: u32,
    base_fee_config: TaikoAnchor::BaseFeeConfig,
    nonce: u64,
    gas_limit: u64,
    max_fee_per_gas: u128,
    max_priority_fee_per_gas: u128,
) -> Result<TxEnvelope, k256::ecdsa::Error> {
    let anchor_call =
        create_anchor_call(anchor_block_id, anchor_state_root, parent_gas_used, base_fee_config);
    let anchor_tx = create_anchor_transaction(
        chain_id,
        nonce,
        gas_limit,
        max_fee_per_gas,
        max_priority_fee_per_gas,
        get_taiko_anchor_address(),
        anchor_call,
    );

    let signed_anchor_tx = get_signed_with_golden_touch(anchor_tx.clone())?;
    Ok(TxEnvelope::from(signed_anchor_tx))
}
