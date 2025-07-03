use std::{num::TryFromIntError, sync::Arc};

use alloy_rpc_types_eth::Block;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{error, info};

use crate::{taiko::taiko_l1_client::ITaikoL1Client, util::ValidTimestamp};

#[derive(Debug, Error)]
pub enum ConfirmationError {
    #[error("{0}")]
    BlobEncode(#[from] crate::blob::BlobEncodeError),

    #[error("{0}")]
    Compression(#[from] libdeflater::CompressionError),

    #[error("{0}")]
    Signer(#[from] alloy_signer::Error),

    #[error("{0}")]
    SolTypes(#[from] alloy_sol_types::Error),

    #[error("{0}")]
    TryU8FromU64(#[from] TryFromIntError),

    #[error("{0}")]
    TaikoL1Client(#[from] crate::taiko::taiko_l1_client::TaikoL1ClientError),

    #[error("Empty block (id={0})")]
    EmptyBlock(u64),
}

pub type ConfirmationResult<T> = Result<T, ConfirmationError>;

#[derive(Debug, Clone)]
pub struct BlockConstrainedConfirmationStrategy<Client: ITaikoL1Client> {
    client: Client,
    blocks: Arc<RwLock<Vec<Block>>>,
    max_blocks: usize,
    valid_timestamp: ValidTimestamp,
}

impl<Client: ITaikoL1Client> BlockConstrainedConfirmationStrategy<Client> {
    pub fn new(
        client: Client,
        blocks: Arc<RwLock<Vec<Block>>>,
        max_blocks: usize,
        valid_timestamp: ValidTimestamp,
    ) -> Self {
        Self {
            client,
            blocks,
            max_blocks,
            valid_timestamp,
        }
    }

    pub async fn send(&self, l1_slot_timestamp: u64, force_send: bool) -> ConfirmationResult<()> {
        info!("send force={force_send}");
        info!("blocks: {}", self.blocks.read().await.len());
        self.blocks.write().await.retain(|block| {
            self.valid_timestamp
                .check(l1_slot_timestamp, block.header.timestamp, 0, 0)
        });
        info!(
            "after removing outdated blocks: {}",
            self.blocks.read().await.len()
        );
        let blocks: Vec<Block> = self
            .blocks
            .read()
            .await
            .iter()
            .filter_map(|block| {
                if block.header.timestamp < l1_slot_timestamp {
                    Some(block.clone())
                } else {
                    None
                }
            })
            .collect();
        info!("after removing too new blocks: {}", blocks.len());
        if blocks.is_empty() || (blocks.len() < self.max_blocks && !force_send) {
            return Ok(());
        }

        self.client.send(blocks).await?;
        Ok(())
    }
}
