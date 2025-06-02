use alloy_consensus::Header as ConsensusHeader;
use alloy_primitives::FixedBytes;
use alloy_rpc_types::Header;

pub fn get_rpc_header(inner: ConsensusHeader) -> Header {
    Header {
        hash: FixedBytes::<32>::default(),
        inner,
        total_difficulty: None,
        size: None,
    }
}

pub fn get_header(number: u64, timestamp: u64) -> ConsensusHeader {
    ConsensusHeader {
        number,
        timestamp,
        ..Default::default()
    }
}
