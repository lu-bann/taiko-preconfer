use std::sync::Arc;

use alloy::rpc::types::eth::Block;
use tokio::sync::RwLock;
use tracing::warn;

#[derive(Debug, Clone)]
pub struct TxCache {
    unconfirmed_blocks: Arc<RwLock<Vec<Block>>>,
}

impl TxCache {
    pub fn new(blocks: Vec<Block>) -> Self {
        Self {
            unconfirmed_blocks: Arc::new(blocks.into()),
        }
    }

    pub async fn blocks(&self) -> Vec<Block> {
        self.unconfirmed_blocks.read().await.clone()
    }

    pub async fn add_block(&mut self, block: Block) {
        if block.transactions.is_empty() {
            warn!("Ignoring empty block {}", block.header.number);
        }
        let mut unconfirmed = self.unconfirmed_blocks.write().await;
        if let Some(current) = unconfirmed
            .iter_mut()
            .find(|unconfirmed_block| unconfirmed_block.header.number == block.header.number)
        {
            *current = block;
        } else {
            unconfirmed.push(block);
        }
        unconfirmed.sort_by(|a, b| a.header.number.cmp(&b.header.number));
    }

    pub async fn retain<F: FnMut(&Block) -> bool>(&mut self, f: F) {
        self.unconfirmed_blocks.write().await.retain(f);
    }

    pub async fn clear(&mut self) {
        self.unconfirmed_blocks.write().await.clear();
    }
}
