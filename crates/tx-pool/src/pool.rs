use std::{sync::Arc, time::Duration};

use alloy_consensus::TxEnvelope;
use parking_lot::RwLock;
use tokio::time::interval;

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

    // TODO: Use a channel to receive updated params
    /// Starts polling the mempool for transactions at a specified interval.
    pub fn start_polling(self: Arc<Self>, params: TxPoolContentParams) {
        tokio::spawn(async move {
            // Note: 2000ms is Taiko's block time
            let mut ticker = interval(Duration::from_millis(2000));
            loop {
                ticker.tick().await;

                match self.client.fetch_mempool_txs(params.clone()).await {
                    Ok(tx_lists) => {
                        let mut pool = self.pool_data.write();
                        pool.clear();
                        pool.extend(tx_lists);
                    }
                    Err(e) => {
                        eprintln!("Polling failed: {:?}", e);
                    }
                }
            }
        });
    }
}

#[derive(Debug, Clone)]
pub struct TxList {
    /// List of transactions
    pub txs: Vec<TxEnvelope>,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use tokio::time::sleep;

    use super::*;
    use crate::mock_txpool_fetcher::MockTxPoolFetcher;

    #[tokio::test]
    async fn test_txpool_polling() {
        let mock_client = Arc::new(MockTxPoolFetcher);
        let tx_pool = TxPool::new(mock_client);

        let params = TxPoolContentParams {
            beneficiary: alloy_primitives::Address::from_str(
                "0xA6f54d514592187F0aE517867466bfd2CCfde4B0",
            )
            .unwrap(),
            base_fee: 10000,
            block_max_gas_limit: 241000000,
            max_bytes_per_tx_list: 10000,
            locals: vec![],
            max_transactions_lists: 10000,
            min_tip: None,
        };

        tx_pool.clone().start_polling(params);

        sleep(Duration::from_secs(1)).await;

        let pool_data = tx_pool.pool_data.read();
        assert!(!pool_data.is_empty(), "Pool data should not be empty");
    }
}
