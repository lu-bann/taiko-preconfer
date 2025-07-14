use alloy::consensus::{Header, TxEnvelope, TxLegacy, TypedTransaction, transaction::Recovered};
use alloy::primitives::{Address, Bytes, FixedBytes, TxKind, U256, address};
use alloy::rpc::types::Header as RpcHeader;
use alloy::rpc::types::eth::{Block, BlockTransactions, Transaction, TransactionInfo};
use k256::ecdsa::SigningKey;

use crate::taiko::sign::{get_signing_key, sign_anchor_tx};

const DUMMY_SIGNER_ADDRESS: Address = address!("0x1670090000000000000000000000000000010001");

pub fn get_test_signing_key() -> SigningKey {
    get_signing_key("0x92954368afd3caa1f3ce3ead0069c1af414054aefe1ef9aeacc1bf426222ce38")
}

pub fn get_dummy_transaction(nonce: u64) -> TypedTransaction {
    TypedTransaction::from(TxLegacy {
        chain_id: Some(167009u64),
        nonce,
        gas_price: 0u128,
        gas_limit: 0u64,
        to: TxKind::Call(DUMMY_SIGNER_ADDRESS),
        value: U256::default(),
        input: Bytes::default(),
    })
}

pub fn get_dummy_signed_transaction(nonce: u64) -> Transaction<TxEnvelope> {
    let tx = get_dummy_transaction(nonce);
    let signing_key = get_test_signing_key();
    let signature = sign_anchor_tx(&signing_key, &tx).unwrap();
    let tx = TxEnvelope::new_unhashed(tx, signature);
    Transaction::from_transaction(
        Recovered::new_unchecked(tx, DUMMY_SIGNER_ADDRESS),
        TransactionInfo::default(),
    )
}

pub fn get_block(number: u64, timestamp: u64) -> Block {
    Block::new(
        get_rpc_header(get_header(number, timestamp)),
        BlockTransactions::Full(vec![get_dummy_signed_transaction(0)]),
    )
}

pub fn get_block_with_txs(number: u64, timestamp: u64, txs: Vec<Transaction<TxEnvelope>>) -> Block {
    Block::new(
        get_rpc_header(get_header(number, timestamp)),
        BlockTransactions::Full(txs),
    )
}

pub fn get_rpc_header(inner: Header) -> RpcHeader {
    RpcHeader {
        hash: FixedBytes::<32>::default(),
        inner,
        total_difficulty: None,
        size: None,
    }
}

pub fn get_header(number: u64, timestamp: u64) -> Header {
    Header {
        number,
        timestamp,
        ..Default::default()
    }
}
