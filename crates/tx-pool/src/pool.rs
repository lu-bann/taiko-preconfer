use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};

use alloy_consensus::TxEnvelope;
use alloy_primitives::Address;
use alloy_rpc_types_engine::JwtSecret;
use parking_lot::RwLock;
use tokio::time::interval;
use url::Url;

use crate::txpool_fetcher::{TaikoAuthClient, TxPoolContentParams};

/// The [`TxPool`] struct represents a transaction pool that holds pending transactions from the
/// mempool.
#[derive(Debug)]
pub struct TxPool {
    /// maps an eoa to all pending txs
    pub pool_data: RwLock<HashMap<Address, TxList>>,
    /// The client used to fetch transactions from the mempool.
    pub client: TaikoAuthClient,
}

impl TxPool {
    /// Creates a new instance of `TxPool` with the given URL and JWT secret.
    pub fn new(url: Url, jwt: JwtSecret) -> Arc<Self> {
        let client = TaikoAuthClient::new(url, jwt).expect("Failed to create TaikoAuth client");
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
    /// EOA address
    pub account: Address,
    /// List of transactions
    pub txs: VecDeque<TxEnvelope>,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use tokio::time::sleep;

    use super::*;

    #[tokio::test]
    async fn test_txpool_polling() {
        let url = Url::parse("http://37.27.222.77:28551").unwrap();
        let jwt_secret =
            JwtSecret::from_hex("654c8ed1da58823433eb6285234435ed52418fa9141548bca1403cc0ad519432")
                .unwrap();
        let tx_pool = TxPool::new(url, jwt_secret);
        let params = TxPoolContentParams {
            beneficiary: Address::from_str("0xA6f54d514592187F0aE517867466bfd2CCfde4B0").unwrap(),
            base_fee: 10000,
            block_max_gas_limit: 241000000,
            max_bytes_per_tx_list: 10000,
            locals: vec![],
            max_transactions_lists: 10000,
            min_tip: None,
        };

        tx_pool.clone().start_polling(params); // 1 second interval

        sleep(Duration::from_secs(20)).await;

        let pool_data = tx_pool.pool_data.read();
        assert!(!pool_data.is_empty(), "Pool data should not be empty");
    }
}
