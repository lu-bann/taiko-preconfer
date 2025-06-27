use alloy_consensus::{TxEip1559, TypedTransaction};
use alloy_primitives::{Address, ChainId, TxKind, U256};
use alloy_sol_types::SolCall;

use crate::taiko::contracts::TaikoAnchor;

const ANCHOR_GAS_LIMIT: u64 = 1_000_000;

pub fn create_anchor_transaction(
    chain_id: ChainId,
    nonce: u64,
    max_fee_per_gas: u128,
    max_priority_fee_per_gas: u128,
    taiko_anchor_address: Address,
    anchor_tx: TaikoAnchor::anchorV3Call,
) -> TypedTransaction {
    TypedTransaction::from(TxEip1559 {
        chain_id,
        nonce,
        gas_limit: ANCHOR_GAS_LIMIT,
        max_fee_per_gas,
        max_priority_fee_per_gas,
        to: TxKind::Call(taiko_anchor_address),
        value: U256::default(),
        access_list: vec![].into(),
        input: anchor_tx.abi_encode().into(),
    })
}

#[cfg(test)]
mod tests {
    use alloy_consensus::Transaction;
    use alloy_primitives::{Bytes, FixedBytes, address};

    use super::*;
    use crate::{taiko::hekla::get_basefee_config_v2, util::hex_decode};

    #[test]
    fn test_create_anchor_transaction() {
        let taiko_anchor_address = address!("0x1670090000000000000000000000000000010001");
        let my_anchor_call = TaikoAnchor::anchorV3Call {
            _anchorBlockId: 3794466,
            _anchorStateRoot: FixedBytes::from_slice(
                &hex_decode("0xf9c88fc529e27024f8b37ae1633931cc1bb0f772f13f912ae77d6771603b16cb")
                    .unwrap(),
            ),
            _parentGasUsed: 181596,
            _baseFeeConfig: get_basefee_config_v2(),
            _signalSlots: vec![],
        };

        let chain_id: ChainId = 167009;
        let nonce = 135343764;
        let max_fee_per_gas: u128 = 10_000_000u128;
        let max_priority_fee_per_gas = 0u128;
        let anchor_transaction = create_anchor_transaction(
            chain_id,
            nonce,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            taiko_anchor_address,
            my_anchor_call,
        );

        let expected_hex_str = "0x48080a45000000000000000000000000000000000000000000000000000000000039e622f9c88fc529e27024f8b37ae1633931cc1bb0f772f13f912ae77d6771603b16cb000000000000000000000000000000000000000000000000000000000002c55c0000000000000000000000000000000000000000000000000000000000000008000000000000000000000000000000000000000000000000000000000000003200000000000000000000000000000000000000000000000000000000004c4b4000000000000000000000000000000000000000000000000000000000502989660000000000000000000000000000000000000000000000000000000023c3460000000000000000000000000000000000000000000000000000000000000001200000000000000000000000000000000000000000000000000000000000000000";
        let expected_input = Bytes::copy_from_slice(&hex_decode(expected_hex_str).unwrap());
        assert_eq!(*anchor_transaction.input(), expected_input);
        assert_eq!(
            anchor_transaction.eip1559().unwrap().to,
            TxKind::Call(taiko_anchor_address)
        )
    }
}
