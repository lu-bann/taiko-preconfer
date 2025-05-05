use alloy_consensus::TxEnvelope;
use alloy_eips::Decodable2718;
use async_trait::async_trait;

use crate::txpool_fetcher::{TxList, TxPoolContentParams, TxPoolFetcher};
pub struct MockTxPoolFetcher;

#[async_trait]
impl TxPoolFetcher for MockTxPoolFetcher {
    async fn fetch_mempool_txs(&self, _params: TxPoolContentParams) -> eyre::Result<Vec<TxList>> {
        let raw_tx = alloy_primitives::hex::decode("02f86f0102843b9aca0085029e7822d68298f094d9e1459a7a482635700cbc20bbaf52d495ab9c9680841b55ba3ac080a0c199674fcb29f353693dd779c017823b954b3c69dffa3cd6b2a6ff7888798039a028ca912de909e7e6cdef9cdcaf24c54dd8c1032946dfa1d85c206b32a9064fe8").unwrap();
        let transaction = TxEnvelope::decode_2718(&mut raw_tx.as_slice()).unwrap();

        Ok(vec![TxList { txs: vec![transaction] }])
    }
}
