use alloy_consensus::{Header, TxEnvelope};
use alloy_primitives::FixedBytes;
use alloy_rpc_types::Header as RpcHeader;
use alloy_rpc_types_eth::{Block, BlockTransactions, Transaction};

pub fn get_block(number: u64, timestamp: u64) -> Block {
    Block::empty(get_rpc_header(get_header(number, timestamp)))
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
