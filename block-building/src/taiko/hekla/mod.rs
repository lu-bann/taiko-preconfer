pub mod addresses;

pub const CHAIN_ID: alloy_primitives::ChainId = 167009;
pub const GAS_LIMIT: u64 = 241_000_000;

use crate::taiko::contracts::TaikoAnchor;
pub fn get_basefee_config_v2() -> TaikoAnchor::BaseFeeConfig {
    TaikoAnchor::BaseFeeConfig {
        adjustmentQuotient: 8,
        sharingPctg: 50,
        gasIssuancePerSecond: 5_000_000,
        minGasExcess: 1_344_899_430,
        maxGasIssuancePerBlock: 600_000_000,
    }
}
