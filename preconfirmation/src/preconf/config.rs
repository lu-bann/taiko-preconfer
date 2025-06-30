use std::{
    env,
    str::FromStr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

use alloy_primitives::Address;

use crate::secret::Secret;

#[derive(Debug, Clone)]
pub struct Config {
    pub l1_genesis_time: SystemTime,
    pub l1_slot_time: Duration,
    pub l1_slots_per_epoch: u64,
    pub l2_slot_time: Duration,
    pub handover_window_slots: u32,
    pub handover_start_buffer: Duration,
    pub anchor_id_lag: u64,
    pub l2_client_url: String,
    pub l2_preconfirmation_url: String,
    pub l2_auth_client_url: String,
    pub l2_ws_url: String,
    pub l1_client_url: String,
    pub l1_ws_url: String,
    pub poll_period: Duration,
    pub taiko_anchor_address: Address,
    pub taiko_inbox_address: Address,
    pub taiko_preconf_router_address: Address,
    pub taiko_whitelist_address: Address,
    pub golden_touch_address: Address,
    pub golden_touch_private_key: String,
    pub private_key: Secret,
    pub jwt_secret: Secret,
    pub anchor_id_update_tol: u64,
    pub max_blocks_per_batch: usize,
    pub use_blobs: bool,
}

#[derive(Debug, PartialEq, Error)]
pub enum ConfigError {
    #[error("{0}")]
    Var(#[from] std::env::VarError),

    #[error("{0}")]
    ParseInt(#[from] std::num::ParseIntError),

    #[error("{0}")]
    ParseBool(#[from] std::str::ParseBoolError),

    #[error("{0}")]
    FromHex(#[from] alloy_primitives::hex::FromHexError),
}

impl Config {
    pub fn try_from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            l1_genesis_time: UNIX_EPOCH
                + Duration::from_secs(env::var("L1_GENESIS_TIMESTAMP_S")?.parse()?),
            l1_slot_time: Duration::from_millis(std::env::var("L1_SLOT_TIME_MS")?.parse()?),
            l1_slots_per_epoch: env::var("L1_SLOTS_PER_EPOCH")?.parse()?,
            l2_slot_time: Duration::from_millis(std::env::var("L2_SLOT_TIME_MS")?.parse()?),
            handover_window_slots: std::env::var("HANDOVER_WINDOW_SLOTS")?.parse()?,
            handover_start_buffer: Duration::from_millis(
                std::env::var("HANDOVER_WINDOW_START_BUFFER_MS")?.parse()?,
            ),
            anchor_id_lag: std::env::var("ANCHOR_ID_LAG")?.parse()?,
            l2_client_url: env::var("L2_CLIENT_URL")?,
            l2_preconfirmation_url: env::var("L2_PRECONFIRMATION_URL")?,
            l2_auth_client_url: env::var("L2_AUTH_CLIENT_URL")?,
            l2_ws_url: env::var("L2_WS_URL")?,
            l1_client_url: env::var("L1_CLIENT_URL")?,
            l1_ws_url: env::var("L1_WS_URL")?,
            poll_period: Duration::from_millis(std::env::var("POLL_TIME_MS")?.parse()?),
            taiko_anchor_address: Address::from_str(&std::env::var("TAIKO_ANCHOR_ADDRESS")?)?,
            taiko_inbox_address: Address::from_str(&std::env::var("TAIKO_INBOX_ADDRESS")?)?,
            taiko_preconf_router_address: Address::from_str(&std::env::var(
                "TAIKO_PRECONF_ROUTER_ADDRESS",
            )?)?,
            taiko_whitelist_address: Address::from_str(&std::env::var("TAIKO_WHITELIST_ADDRESS")?)?,
            golden_touch_address: Address::from_str(&std::env::var("GOLDEN_TOUCH_ADDRESS")?)?,
            golden_touch_private_key: std::env::var("GOLDEN_TOUCH_PRIVATE_KEY")?,
            private_key: Secret::new(std::env::var("PRIVATE_KEY")?),
            jwt_secret: Secret::new(std::env::var("JWT_SECRET")?),
            anchor_id_update_tol: std::env::var("ANCHOR_ID_UPDATE_TOL")?.parse()?,
            max_blocks_per_batch: std::env::var("MAX_BLOCKS_PER_BATCH")?.parse()?,
            use_blobs: std::env::var("USE_BLOBS")?.parse()?,
        })
    }
}
