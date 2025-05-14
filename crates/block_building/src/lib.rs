pub mod base_fee;
pub mod taiko;
pub mod util;
pub mod verification;

pub const L2_BLOCK_TIME_MS: u64 = 2000;

use taiko::contracts::TaikoAnchor::BaseFeeConfig;
// Values taken from https://github.com/taikoxyz/taiko-mono/blob/main/packages/protocol/contracts/layer1/hekla/HeklaInbox.sol#L94
pub const BASE_FEE_CONFIG_HEKLA: BaseFeeConfig = BaseFeeConfig {
    adjustmentQuotient: 8,
    sharingPctg: 50,
    gasIssuancePerSecond: 5_000_000,
    minGasExcess: 1_344_899_430,
    maxGasIssuancePerBlock: 600_000_000,
};
