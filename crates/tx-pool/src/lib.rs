pub mod txpool_fetcher;

pub const BLOCK_MAX_GAS_LIMIT: u64 = 241_000_000;
// Ref: https://github.com/taikoxyz/taiko-mono/blob/b62731c4df09701c1d8a0a4449d3cd5001e08c48/packages/taiko-client/pkg/rpc/utils.go#L28
pub const MAX_BYTES_PER_TX_LIST: u64 = 126_976; // 31 * 4096
// ref https://github.com/taikoxyz/taiko-mono/blob/b62731c4df09701c1d8a0a4449d3cd5001e08c48/packages/taiko-client/proposer/config.go#L76
pub const MAX_TRANSACTIONS_LISTS: u8 = 6;

pub const TX_POOL_CONTENT_METHOD: &str = "taikoAuth_txPoolContent";
pub const TX_POOL_CONTENT_WITH_MIN_TIP_METHOD: &str = "taikoAuth_txPoolContentWithMinTip";
