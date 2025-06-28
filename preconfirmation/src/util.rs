use alloy_consensus::TxEnvelope;
use alloy_primitives::Bytes;
use alloy_rpc_types_eth::Block;
use alloy_sol_types::SolCall;
use hex::{FromHexError, decode, encode};
use tracing::error;

use crate::taiko::contracts::TaikoAnchor;

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

pub fn get_anchor_block_id_from_bytes(bytes: &Bytes) -> Result<u64, alloy_sol_types::Error> {
    let anchor_v3_call = TaikoAnchor::anchorV3Call::abi_decode(bytes)?;
    Ok(anchor_v3_call._anchorBlockId)
}

pub fn get_tx_envelopes_without_anchor_from_block(block: Block) -> Vec<TxEnvelope> {
    let mut txs = get_tx_envelopes_from_block(block);
    txs.remove(0);
    txs
}

pub fn get_tx_envelopes_without_anchor_from_blocks(txs: Vec<Block>) -> Vec<TxEnvelope> {
    txs.into_iter()
        .flat_map(get_tx_envelopes_without_anchor_from_block)
        .collect()
}

pub fn pad_left<const N: usize>(bytes: &[u8]) -> Bytes {
    let mut padded = [0u8; N];

    let start = N - bytes.len();
    padded[start..].copy_from_slice(bytes);

    Bytes::from(padded)
}

pub fn parse_transport_error(err: alloy_transport::TransportError) -> String {
    match err {
        alloy_transport::TransportError::ErrorResp(alloy_json_rpc::ErrorPayload {
            code,
            message,
            data,
        }) => {
            if let Some(data) = data {
                let data_str = data.to_string();
                parse_taiko_sol_error(&data_str[1..(data_str.len() - 1)])
            } else {
                format!("{code} {message}")
            }
        }
        err => err.to_string(),
    }
}

pub fn parse_taiko_sol_error(code: &str) -> String {
    match code {
        "0x7f14daf8" => "AnchorBlockIdTooLarge()",
        "0x46afbf54" => "AnchorBlockIdTooSmall()",
        "0x83c7eca6" => "ArraySizesMismatch()",
        "0x9e15e1bc" => "BatchNotFound()",
        "0xc63c8bfd" => "BatchVerified()",
        "0x8879ee78" => "BeyondCurrentFork()",
        "0xf765f45e" => "BlobNotFound()",
        "0xfeb32ebd" => "BlockNotFound()",
        "0xf911438d" => "BlobNotSpecified()",
        "0xab35696f" => "ContractPaused()",
        "0x10213ad7" => "CustomProposerMissing()",
        "0xc25c48e2" => "CustomProposerNotAllowed()",
        "0x018ad7d6" => "EtherNotPaidAsBond()",
        "0x324d6078" => "FirstBlockTimeShiftNotZero()",
        "0x0db26169" => "ForkNotActivated()",
        "0xe92c469f" => "InsufficientBond()",
        "0x0f49f066" => "InvalidBlobCreatedIn()",
        "0x2677ebff" => "InvalidBlobParams()",
        "0xcd21cd43" => "InvalidGenesisBlockHash()",
        "0xa86b6512" => "InvalidParams()",
        "0xac97cfc7" => "InvalidTransitionBlockHash()",
        "0x19ead341" => "InvalidTransitionParentHash()",
        "0x6c0118eb" => "InvalidTransitionStateRoot()",
        "0x419b53b7" => "MetaHashMismatch()",
        "0x798ee6f1" => "MsgValueNotZero()",
        "0x367b91ca" => "NoBlocksToProve()",
        "0x09e29fae" => "NotFirstProposal()",
        "0x5e9e4449" => "NotInboxWrapper()",
        "0xff626f77" => "ParentMetaHashMismatch()",
        "0xed6222ff" => "SameTransition()",
        "0x8601a5fe" => "SignalNotSent()",
        "0x21389b84" => "TimestampSmallerThanParent()",
        "0x3d32ffdb" => "TimestampTooLarge()",
        "0x1999aed2" => "TimestampTooSmall()",
        "0xa464214b" => "TooManyBatches()",
        "0x7f06d57a" => "TooManyBlocks()",
        "0xc577d383" => "TooManySignals()",
        "0x76be38bc" => "TransitionNotFound()",
        "0x2b44f010" => "ZeroAnchorBlockHash()",
        other => other,
    }
    .to_string()
}

pub fn is_not_outdated_timestamp(
    last_l2_block_timestamp: u64,
    l1_block_timestamp: u64,
    previous_batch_timestamp: u64,
    total_shift: u64,
    max_offset: u64,
) -> bool {
    last_l2_block_timestamp >= previous_batch_timestamp
        && last_l2_block_timestamp >= total_shift
        && last_l2_block_timestamp + max_offset >= l1_block_timestamp + total_shift
}

#[derive(Debug, Clone)]
pub struct ValidTimestamp {
    max_offset: u64,
}

impl ValidTimestamp {
    pub const fn new(max_offset: u64) -> Self {
        Self { max_offset }
    }

    pub fn check(
        &self,
        l1_block_timestamp: u64,
        last_l2_block_timestamp: u64,
        previous_batch_timestamp: u64,
        total_shift: u64,
    ) -> bool {
        is_not_outdated_timestamp(
            last_l2_block_timestamp,
            l1_block_timestamp,
            previous_batch_timestamp,
            total_shift,
            self.max_offset,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_valid_anchor_id() {
        let input = hex_decode("0x48080a4500000000000000000000000000000000000000000000000000000000003e3664a1fb40892d9c1a27e5c8c8cc4e48eb87e70f82efdbaa5b1323b65f2e1e3a8e3b000000000000000000000000000000000000000000000000000000000002d61c0000000000000000000000000000000000000000000000000000000000000008000000000000000000000000000000000000000000000000000000000000003200000000000000000000000000000000000000000000000000000000004c4b4000000000000000000000000000000000000000000000000000000000502989660000000000000000000000000000000000000000000000000000000023c3460000000000000000000000000000000000000000000000000000000000000001200000000000000000000000000000000000000000000000000000000000000000").unwrap();
        let bytes = Bytes::from(input);
        let anchor_id = get_anchor_block_id_from_bytes(&bytes).unwrap();
        let expected_anchor_id = 4077156;
        assert_eq!(anchor_id, expected_anchor_id);
    }

    #[test]
    fn test_last_block_timestamp_is_invalid_if_before_last_batch_timestamp() {
        let last_l2_block_timestamp = 1;
        let last_batch_timestamp = 2;
        let valid =
            is_not_outdated_timestamp(last_l2_block_timestamp, 0, last_batch_timestamp, 0, 0);
        assert!(!valid);
    }

    #[test]
    fn test_last_block_timestamp_is_invalid_if_smaller_than_time_shift() {
        let last_l2_block_timestamp = 1;
        let last_batch_timestamp = 2;
        let total_shift = 2;
        let valid = is_not_outdated_timestamp(
            last_l2_block_timestamp,
            0,
            last_batch_timestamp,
            total_shift,
            0,
        );
        assert!(!valid);
    }

    #[test]
    fn test_last_block_timestamp_is_invalid_if_more_than_max_offset_behind_current_block() {
        let last_l2_block_timestamp = 3;
        let last_batch_timestamp = 2;
        let total_shift = 2;
        let l1_block_timestamp = 12;
        let max_offset = 10;
        let valid = is_not_outdated_timestamp(
            last_l2_block_timestamp,
            l1_block_timestamp,
            last_batch_timestamp,
            total_shift,
            max_offset,
        );
        assert!(!valid);
    }

    #[test]
    fn test_valid_last_block_timestamp() {
        let last_l2_block_timestamp = 3;
        let last_batch_timestamp = 2;
        let total_shift = 2;
        let l1_block_timestamp = 11;
        let max_offset = 10;
        let valid = is_not_outdated_timestamp(
            last_l2_block_timestamp,
            l1_block_timestamp,
            last_batch_timestamp,
            total_shift,
            max_offset,
        );
        assert!(valid);
    }

    #[test]
    fn test_convert_sol_error() {
        let err = "0x1999aed2";
        let converted = parse_taiko_sol_error(err);
        assert_eq!(converted, "TimestampTooSmall()".to_string());
    }

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
