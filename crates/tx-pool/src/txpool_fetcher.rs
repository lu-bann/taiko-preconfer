use alloy_consensus::TxEnvelope;
use alloy_primitives::{Address, Bytes};
use alloy_rpc_client::RpcClient;
use alloy_rpc_types_engine::JwtSecret;
use alloy_transport_http::{
    AuthLayer, Http, HyperClient,
    hyper_util::{client::legacy::Client, rt::TokioExecutor},
};
use async_trait::async_trait;
use http_body_util::Full;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower::ServiceBuilder;

use crate::pool::TxList;

#[async_trait]
pub trait TxPoolFetcher: Send + Sync {
    /// Fetches mempool transactions through the `taikoAuth_txPoolContent` RPC method.
    async fn fetch_mempool_txs(&self, params: TxPoolContentParams) -> eyre::Result<Vec<TxList>>;
}

/// The [`TaikoAuthClient`] is responsible for interacting with the taikoAuth API via HTTP.
/// The inner transport uses a JWT [AuthLayer] to authenticate requests.
#[derive(Clone)]
pub struct TaikoAuthClient {
    pub inner: RpcClient,
}

impl TaikoAuthClient {
    /// Creates a new [`TaikoAuthClient`] from the provided [Url] and [JwtSecret].
    pub fn new(url: Url, jwt_secret: JwtSecret) -> eyre::Result<Self> {
        let hyper_client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();
        let auth_layer = AuthLayer::new(jwt_secret);
        let service = ServiceBuilder::new().layer(auth_layer).service(hyper_client);

        let layer_transport = HyperClient::with_service(service);
        let http_hyper = Http::with_client(layer_transport, url);

        Ok(Self { inner: RpcClient::new(http_hyper, true) })
    }
}

#[async_trait]
impl TxPoolFetcher for TaikoAuthClient {
    async fn fetch_mempool_txs(&self, params: TxPoolContentParams) -> eyre::Result<Vec<TxList>> {
        let params = vec![
            json!(params.beneficiary),
            json!(params.base_fee),
            json!(params.block_max_gas_limit),
            json!(params.max_bytes_per_tx_list),
            json!(params.locals),
            json!(params.max_transactions_lists),
        ];

        let rpc_call = self.inner.request("taikoAuth_txPoolContent", params);
        let response: Vec<PreBuiltTxList> = rpc_call.await?;

        Ok(response.into_iter().map(|prebuilt_tx_list| prebuilt_tx_list.into()).collect())
    }
}

/// PreBuiltTxList is a pre-built transaction list based on the latest chain state,
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PreBuiltTxList {
    pub tx_list: Vec<TxEnvelope>,
    pub estimated_gas_used: u64,
    pub bytes_length: u64,
}

impl From<PreBuiltTxList> for TxList {
    fn from(value: PreBuiltTxList) -> Self {
        TxList { txs: value.tx_list }
    }
}

/// Parameters for the `taikoAuth_txPoolContent` & `taikoAuth_txPoolContentWithMinTip` RPC methods.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TxPoolContentParams {
    /// Coinbase address
    pub beneficiary: Address,
    /// Base fee
    pub base_fee: u64,
    /// Block max gas limit
    pub block_max_gas_limit: u64,
    /// Max bytes per transaction list
    pub max_bytes_per_tx_list: u64,
    /// List of local addresses
    pub locals: Vec<String>,
    /// Max transactions lists
    pub max_transactions_lists: u64,
    /// Minimum tip
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_tip: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code)]
    #[derive(Debug, Deserialize)]
    struct JsonRpcResponse<T> {
        jsonrpc: String,
        id: u64,
        result: T,
    }

    #[tokio::test]
    async fn test_get_mempool_txs() -> eyre::Result<()> {
        let instant = std::time::Instant::now();

        let json_str = std::fs::read_to_string("../../tests/data/txpool_content.json")?;
        let rpc_response: JsonRpcResponse<Vec<PreBuiltTxList>> = serde_json::from_str(&json_str)?;

        let mempool_txs: Vec<PreBuiltTxList> = rpc_response.result.into_iter().collect();
        let elapsed = instant.elapsed().as_millis();
        println!("Mempool Transactions: {:?}", mempool_txs);
        println!("Elapsed time: {:?}ms", elapsed);
        let total_txs: usize = mempool_txs.iter().map(|tx_list| tx_list.tx_list.len()).sum();
        println!("Total transactions: {}", total_txs);
        Ok(())
    }
}
