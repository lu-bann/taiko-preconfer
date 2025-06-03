use alloy_primitives::{Address, FixedBytes};
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct BlockMetadata {
    pub beneficiary: Address,
    pub gas_limit: u64,
    pub timestamp: u64,
    pub mix_hash: FixedBytes<32>,

    pub tx_list: Vec<u8>, 
    pub extra_data: Vec<u8>,
}

impl BlockMetadata {
    pub fn new(
        beneficiary: Address,
        gas_limit: u64,
        timestamp: u64,
        mix_hash: FixedBytes<32>,
        tx_list: Vec<u8>,
        extra_data: Vec<u8>,
    ) -> Self {
        return Self {
            beneficiary,
            gas_limit,
            timestamp,
            mix_hash,
            tx_list,
            extra_data,
        }
    }
}
