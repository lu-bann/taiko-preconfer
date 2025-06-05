use std::{
    env,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

use crate::slot_model::HOLESKY_GENESIS_TIMESTAMP;

#[derive(Debug)]
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
    pub taiko_anchor_address: String,
    pub taiko_inbox_address: String,
    pub taiko_wrapper_address: String,
    pub golden_touch_address: String,
    pub golden_touch_private_key: String,
}

#[derive(Debug, PartialEq, Error)]
pub enum ConfigError {
    #[error("{0}")]
    Var(#[from] std::env::VarError),

    #[error("{0}")]
    Parse(#[from] std::num::ParseIntError),
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
            taiko_anchor_address: std::env::var("TAIKO_ANCHOR_ADDRESS")?,
            taiko_inbox_address: std::env::var("TAIKO_INBOX_ADDRESS")?,
            taiko_wrapper_address: std::env::var("TAIKO_WRAPPER_ADDRESS")?,
            golden_touch_address: std::env::var("GOLDEN_TOUCH_ADDRESS")?,
            golden_touch_private_key: std::env::var("GOLDEN_TOUCH_PRIVATE_KEY")?,
        })
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            l1_genesis_time: UNIX_EPOCH + Duration::from_secs(HOLESKY_GENESIS_TIMESTAMP),
            l1_slot_time: Duration::from_millis(12000),
            l1_slots_per_epoch: 32,
            l2_slot_time: Duration::from_millis(2000),
            handover_window_slots: 4,
            handover_start_buffer: Duration::from_millis(6000),
            anchor_id_lag: 4,
            l2_client_url: String::default(),
            l2_preconfirmation_url: String::default(),
            l2_auth_client_url: String::default(),
            l2_ws_url: String::default(),
            l1_client_url: String::default(),
            l1_ws_url: String::default(),
            poll_period: Duration::from_millis(50),
            taiko_anchor_address: "0x1670090000000000000000000000000000010001".into(),
            taiko_inbox_address: "0x79C9109b764609df928d16fC4a91e9081F7e87DB".into(),
            taiko_wrapper_address: "0xD3f681bD6B49887A48cC9C9953720903967E9DC0".into(),
            golden_touch_address: "0x0000777735367b36bC9B61C50022d9D0700dB4Ec".into(),
            golden_touch_private_key:
                "0x92954368afd3caa1f3ce3ead0069c1af414054aefe1ef9aeacc1bf426222ce38".into(),
        }
    }
}
