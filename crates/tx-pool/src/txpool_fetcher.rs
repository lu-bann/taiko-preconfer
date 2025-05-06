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

#[derive(Debug, Clone)]
pub struct TxList {
    /// List of transactions
    pub txs: Vec<TxEnvelope>,
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

    #[test]
    fn test_get_mempool_txs() {
        let json_str = r#"
        [
            {
                "TxList": [
                    {
                        "type": "0x0",
                        "chainId": "0x28c61",
                        "nonce": "0x8",
                        "to": "0x0011e559da84dde3f841e22dc33f3adbf184d84a",
                        "gas": "0xe51ec90",
                        "gasPrice": "0x59682f01",
                        "maxPriorityFeePerGas": null,
                        "maxFeePerGas": null,
                        "value": "0x0",
                        "input": "0xa9059cbb00000000000000000000000097d4677accc7b656b5eb8afcdafe6406c1f5597f000000000000000000000000000000000000000000000000000000000010545a",
                        "v": "0x518e5",
                        "r": "0x6564a46a92d69d11bd873b901fced6e9238824cc9ffc01aafa2c8f2391ef089",
                        "s": "0x55c8c647086fe59b314e45c5770e61375a4dc7f5a41a70bc8c919efa893dfcea",
                        "hash": "0x3f89cf8247391294cffa25a38517cdb000ea7eb80d66a491bb3bfd37fa96b708"
                    }
                ],
                "EstimatedGasUsed": 0,
                "BytesLength": 162
            }
        ]"#;

        let mempool_txs: Vec<PreBuiltTxList> = serde_json::from_str(json_str).unwrap();
        let total_txs: usize = mempool_txs.iter().map(|tx_list| tx_list.tx_list.len()).sum();
        assert_eq!(total_txs, 1);
    }
}
