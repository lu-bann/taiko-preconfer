use std::num::TryFromIntError;

use alloy_consensus::Transaction;
use alloy_rpc_types_eth::Block;
use thiserror::Error;
use tracing::debug;

use crate::{
    taiko::taiko_l1_client::ITaikoL1Client,
    tx_cache::TxCache,
    util::{ValidTimestamp, get_anchor_block_id_from_bytes},
};

#[derive(Debug, Error)]
pub enum ConfirmationError {
    #[error("{0}")]
    Kzg(#[from] c_kzg::Error),

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

    #[error("Missing anchor tx in block {0}")]
    MissingAnchor(u64),
}

pub type ConfirmationResult<T> = Result<T, ConfirmationError>;

#[derive(Debug, Clone)]
pub struct BlockConstrainedConfirmationStrategy<Client: ITaikoL1Client> {
    client: Client,
    tx_cache: TxCache,
    max_blocks: usize,
    valid_timestamp: ValidTimestamp,
}

impl<Client: ITaikoL1Client> BlockConstrainedConfirmationStrategy<Client> {
    pub fn new(
        client: Client,
        tx_cache: TxCache,
        max_blocks: usize,
        valid_timestamp: ValidTimestamp,
    ) -> Self {
        Self {
            client,
            tx_cache,
            max_blocks,
            valid_timestamp,
        }
    }

    pub async fn send(
        &self,
        l1_slot_timestamp: u64,
        force_send: bool,
        current_anchor_id: u64,
    ) -> ConfirmationResult<()> {
        let mut tx_cache = self.tx_cache.clone();
        tx_cache
            .retain(|block| {
                self.valid_timestamp
                    .check(l1_slot_timestamp, block.header.timestamp, 0, 0)
                    && block.header.beneficiary == self.client.address()
            })
            .await;
        let blocks = tx_cache.blocks().await;
        debug!("send force={}, #blocks={}", force_send, blocks.len());
        let blocks: Vec<Block> = blocks
            .iter()
            .filter_map(|block| {
                if block.header.timestamp < l1_slot_timestamp {
                    Some(block.clone())
                } else {
                    None
                }
            })
            .collect();
        if blocks.is_empty() {
            return Ok(());
        }
        let first_anchor_tx = blocks
            .first()
            .expect("Must be present")
            .transactions
            .txns()
            .next();
        if first_anchor_tx.is_none() {
            return Err(ConfirmationError::MissingAnchor(
                blocks.first().expect("Must be present").header.number,
            ));
        }
        let batch_anchor_id: u64 =
            get_anchor_block_id_from_bytes(first_anchor_tx.expect("Must be present").input())?;
        let mut batch_blocks = vec![];
        for block in blocks.iter() {
            let block_anchor_id: u64 = get_anchor_block_id_from_bytes(
                block
                    .transactions
                    .txns()
                    .next()
                    .ok_or(ConfirmationError::MissingAnchor(block.header.number))?
                    .input(),
            )?;
            if block_anchor_id == batch_anchor_id {
                batch_blocks.push(block.clone());
            } else {
                break;
            }
        }
        // force sending it more than one anchor id is used or all new blocks will get another anchor
        let force_send = force_send
            || batch_blocks.len() != blocks.len()
            || batch_anchor_id != current_anchor_id;
        if batch_blocks.is_empty() || (batch_blocks.len() < self.max_blocks && !force_send) {
            return Ok(());
        }

        self.client.send(batch_blocks, batch_anchor_id).await?;
        Ok(())
    }
}
