use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};

use alloy_consensus::TxEnvelope;
use alloy_primitives::Address;
use parking_lot::RwLock;
use tokio::time::interval;

use crate::txpool_fetcher::{TxPoolContentParams, TxPoolFetcher};

/// The [`TxPool`] struct represents a transaction pool that holds pending transactions from the
/// mempool.
pub struct TxPool<C: TxPoolFetcher> {
    /// maps an eoa to all pending txs
    pub pool_data: RwLock<HashMap<Address, TxList>>,
    /// The client used to fetch transactions from the mempool.
    pub client: Arc<C>,
}

impl<C: TxPoolFetcher + 'static> TxPool<C> {
    /// Creates a new instance of [`TxPool`] with the provided client.
    pub fn new(client: Arc<C>) -> Arc<Self> {
        Arc::new(Self { pool_data: RwLock::new(HashMap::new()), client })
    }

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
                        for tx_list in tx_lists {
                            pool.insert(tx_list.account, tx_list);
                        }
                    }
                    Err(e) => {
                        eprintln!("Polling failed: {:?}", e);
                    }
                }
            }
        });
    }
}

/// A nonce-sorted list of transactions from a single sender.
#[derive(Debug, Clone)]
pub struct TxList {
    /// The sender's address.
    pub account: Address,
    /// List of transactions
    pub txs: VecDeque<TxEnvelope>,
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
            beneficiary: Address::from_str("0xA6f54d514592187F0aE517867466bfd2CCfde4B0").unwrap(),
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
