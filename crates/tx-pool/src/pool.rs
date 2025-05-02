use std::sync::Arc;

use alloy_consensus::TxEnvelope;
use parking_lot::RwLock;

use crate::txpool_fetcher::{TxPoolContentParams, TxPoolFetcher};

/// The [`TxPool`] struct represents a tx pool that holds pending txs from the mempool.
pub struct TxPool<C: TxPoolFetcher> {
    /// Txs as batches
    pub pool_data: RwLock<Vec<TxList>>,
    /// The client used to fetch transactions from the mempool.
    pub client: Arc<C>,
}

impl<C: TxPoolFetcher + 'static> TxPool<C> {
    /// Creates a new instance of [`TxPool`] with the provided client.
    pub fn new(client: Arc<C>) -> Arc<Self> {
        Arc::new(Self { pool_data: RwLock::new(Vec::new()), client })
    }

    #[allow(dead_code)]
    async fn fetch_new_txs(&self, params: TxPoolContentParams) -> eyre::Result<Vec<TxList>> {
        // Fetch transactions
        let txs = self.client.fetch_mempool_txs(params).await?;

        // Save transactions to the pool
        let mut pool_data = self.pool_data.write();
        pool_data.clear();
        pool_data.extend(txs.clone());
        Ok(txs)
    }
}

#[derive(Debug, Clone)]
pub struct TxList {
    /// List of transactions
    pub txs: Vec<TxEnvelope>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock_txpool_fetcher::MockTxPoolFetcher;

    #[tokio::test]
    async fn test_txpool_polling() {
        let mock_client = Arc::new(MockTxPoolFetcher);
        let tx_pool = TxPool::new(mock_client);

        let _ = tx_pool.clone().fetch_new_txs(TxPoolContentParams::default()).await;

        let pool_data = tx_pool.pool_data.read();
        assert!(!pool_data.is_empty(), "Pool data should not be empty");
    }
}
