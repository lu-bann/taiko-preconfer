use std::{env, time::Duration};
use thiserror::Error;

#[derive(Debug)]
pub struct Config {
    pub l2_block_time: Duration,
    #[allow(dead_code)]
    pub handover_window_slots: u32,
    #[allow(dead_code)]
    pub handover_start_buffer: Duration,
    pub anchor_id_lag: u64,
    pub l2_client_url: String,
    pub l2_auth_client_url: String,
    pub l2_ws_url: String,
    pub l1_client_url: String,
    pub l1_ws_url: String,
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
            l2_block_time: Duration::from_millis(std::env::var("L2_BLOCK_TIME_MS")?.parse()?),
            handover_window_slots: std::env::var("HANDOVER_WINDOW_SLOTS")?.parse()?,
            handover_start_buffer: Duration::from_millis(
                std::env::var("HANDOVER_WINDOW_START_BUFFER_MS")?.parse()?,
            ),
            anchor_id_lag: std::env::var("ANCHOR_ID_LAG")?.parse()?,
            l2_client_url: env::var("L2_CLIENT_URL")?,
            l2_auth_client_url: env::var("L2_AUTH_CLIENT_URL")?,
            l2_ws_url: env::var("L2_WS_URL")?,
            l1_client_url: env::var("L1_CLIENT_URL")?,
            l1_ws_url: env::var("L1_WS_URL")?,
        })
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            l2_block_time: Duration::from_millis(12000),
            handover_window_slots: 4,
            handover_start_buffer: Duration::from_millis(6000),
            anchor_id_lag: 4,
            l2_client_url: String::default(),
            l2_auth_client_url: String::default(),
            l2_ws_url: String::default(),
            l1_client_url: String::default(),
            l1_ws_url: String::default(),
        }
    }
}
