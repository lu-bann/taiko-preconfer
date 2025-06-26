use alloy_consensus::TxEnvelope;
use alloy_primitives::Bytes;
use alloy_rpc_types_eth::Block;
use tracing::error;

pub fn log_error<T, E: ToString>(result: Result<T, E>, msg: &str) -> Option<T> {
    match result {
        Err(err) => {
            error!("{msg}: {}", err.to_string());
            None
        }
        Ok(value) => Some(value),
    }
}

pub fn get_tx_envelopes_from_block(block: Block) -> Vec<TxEnvelope> {
    block
        .transactions
        .into_transactions_vec()
        .into_iter()
        .map(|tx| tx.inner.into_inner())
        .collect()
}

pub fn get_tx_envelopes_from_blocks(txs: Vec<Block>) -> Vec<TxEnvelope> {
    txs.into_iter()
        .flat_map(get_tx_envelopes_from_block)
        .collect()
}

pub fn pad_left<const N: usize>(bytes: &[u8]) -> Bytes {
    let mut padded = [0u8; N];

    let start = N - bytes.len();
    padded[start..].copy_from_slice(bytes);

    Bytes::from(padded)
}
