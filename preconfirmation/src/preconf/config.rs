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
        }
    }
}
