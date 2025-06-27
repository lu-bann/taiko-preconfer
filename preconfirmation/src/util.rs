use alloy_consensus::TxEnvelope;
use alloy_primitives::Bytes;
use alloy_rpc_types_eth::Block;
use hex::{FromHexError, decode, encode};
use tracing::error;

pub fn hex_encode<T: AsRef<[u8]>>(data: T) -> String {
    format!("0x{}", encode(data))
}

pub fn hex_decode(hex_string: &str) -> Result<Vec<u8>, FromHexError> {
    decode(hex_string.trim_start_matches("0x"))
}

pub fn hex_to_u64(s: &str) -> Result<u64, std::num::ParseIntError> {
    u64::from_str_radix(s.trim_start_matches("0x"), 16)
}

pub fn u64_to_hex(value: u64) -> String {
    format!("{value:#x}")
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_encode() {
        let encoded = hex_encode([15u8]);
        assert_eq!(encoded, String::from("0x0f"));
    }

    #[test]
    fn test_hex_decode() {
        let hex_str = "0x10";
        let decoded = hex_decode(hex_str).unwrap();
        assert_eq!(decoded, vec![16u8]);
    }

    #[test]
    fn test_hex_to_u64() {
        let hex_str = "0x10";
        let decoded = hex_to_u64(hex_str).unwrap();
        assert_eq!(decoded, 16u64);
    }

    #[test]
    fn test_u64_to_hex() {
        let value = 16u64;
        let encoded = u64_to_hex(value);
        assert_eq!(encoded, "0x10");
    }

    #[test]
    fn test_left_pad() {
        let input: Vec<u8> = vec![3, 2, 1];
        let padded = pad_left::<5>(&input);
        let expected = Bytes::from([0, 0, 3, 2, 1]);
        assert_eq!(padded, expected);
    }
}
