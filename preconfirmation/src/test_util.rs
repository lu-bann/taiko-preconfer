use alloy_consensus::Header;
use alloy_primitives::FixedBytes;
use alloy_rpc_types::Header as RpcHeader;

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
