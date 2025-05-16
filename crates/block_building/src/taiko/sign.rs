use alloy_consensus::{SignableTransaction, TypedTransaction};
use alloy_primitives::Signature;
use k256::{
    Scalar, Secp256k1,
    ecdsa::{Error as EcdsaError, SigningKey, hazmat::sign_prehashed},
    elliptic_curve::FieldBytes,
};

fn sign_with_fixed_k(
    signing_key: &SigningKey,
    tx: &TypedTransaction,
    k: u64,
) -> Result<Signature, EcdsaError> {
    let message_hash = FieldBytes::<Secp256k1>::clone_from_slice(tx.signature_hash().as_slice());
    let (signature, recovery_id) = sign_prehashed::<Secp256k1, Scalar>(
        signing_key.as_nonzero_scalar(),
        Scalar::from(k),
        &message_hash,
    )?;
    let mut signature_bytes = signature.normalize_s().unwrap_or(signature).to_vec();
    signature_bytes.push(recovery_id.to_byte());
    Ok(Signature::try_from(signature_bytes.as_slice()).unwrap())
}

pub fn sign_anchor_tx(
    signing_key: &SigningKey,
    tx: &TypedTransaction,
) -> Result<Signature, EcdsaError> {
    sign_with_fixed_k(signing_key, tx, 1u64).or(sign_with_fixed_k(signing_key, tx, 2u64))
}

#[cfg(test)]
mod tests {
    use alloy_consensus::{TxEip1559, TxLegacy};
    use alloy_eips::eip2930::AccessList;
    use alloy_primitives::{Bytes, FixedBytes, TxKind, U256};
    use alloy_sol_types::SolCall;

    use super::*;
    use crate::{
        encode_util::hex_decode,
        taiko::{
            contracts::TaikoAnchor,
            hekla::{
                addresses::{get_golden_touch_signing_key, get_taiko_anchor_address},
                get_basefee_config_v2,
            },
        },
    };

    fn get_test_transaction() -> TypedTransaction {
        TypedTransaction::from(TxLegacy {
            chain_id: Some(167009u64),
            nonce: 0u64,
            gas_price: 0u128,
            gas_limit: 0u64,
            to: TxKind::Call(get_taiko_anchor_address()),
            value: U256::default(),
            input: Bytes::default(),
        })
    }

    fn get_high_s_test_transaction() -> TypedTransaction {
        let anchor_call = TaikoAnchor::anchorV3Call {
            _anchorBlockId: 3839301u64,
            _anchorStateRoot: FixedBytes::<32>::from_slice(
                &hex_decode("0x0299771df8290c0d483a14071b3a1adf485be4d492b172ff76fee982e1eddebd")
                    .unwrap(),
            ),
            _parentGasUsed: 5676556u32,
            _baseFeeConfig: get_basefee_config_v2(),
            _signalSlots: vec![],
        };
        TypedTransaction::from(TxEip1559 {
            chain_id: 167009u64,
            nonce: 1380460u64,
            gas_limit: 1_000_000u64,
            max_fee_per_gas: 10_000_000u128,
            max_priority_fee_per_gas: 0u128,
            to: TxKind::Call(get_taiko_anchor_address()),
            value: U256::ZERO,
            access_list: AccessList::default(),
            input: Bytes::copy_from_slice(&anchor_call.abi_encode()),
        })
    }

    #[test]
    fn signature_with_k_1() {
        let tx = get_test_transaction();
        let signing_key = get_golden_touch_signing_key();
        let k = 1u64;
        let signature = sign_with_fixed_k(&signing_key, &tx, k).unwrap();
        assert_eq!(
            signature.r().to_string(),
            "55066263022277343669578718895168534326250603453777594175500187360389116729240"
        );
        assert_eq!(
            signature.s().to_string(),
            "55606847455169850712614713723130181997308526921030548275028061212750923145154"
        );
        assert!(!signature.v());
    }

    #[test]
    fn normalized_signature_with_k_1() {
        let tx = get_high_s_test_transaction();
        let signing_key = get_golden_touch_signing_key();
        let k = 1u64;
        let signature = sign_with_fixed_k(&signing_key, &tx, k).unwrap();
        assert_eq!(
            signature.r().to_string(),
            "55066263022277343669578718895168534326250603453777594175500187360389116729240"
        );
        assert_eq!(
            signature.s().to_string(),
            "33866923707463146537069755075936353461146384527221206883437412880615995522297"
        );
        assert!(!signature.v());
    }

    #[test]
    fn signature_with_k_2() {
        let tx = get_test_transaction();
        let signing_key = get_golden_touch_signing_key();
        let k = 2u64;
        let signature = sign_with_fixed_k(&signing_key, &tx, k).unwrap();

        assert_eq!(
            signature.r().to_string(),
            "89565891926547004231252920425935692360644145829622209833684329913297188986597"
        );
        assert_eq!(
            signature.s().to_string(),
            "29388298015506984210706315771587267265112769511363726733177929448874781094591"
        );
        assert!(!signature.v());
    }
}
