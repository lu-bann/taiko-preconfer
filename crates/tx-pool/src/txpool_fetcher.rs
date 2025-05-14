use alloy_consensus::TxEnvelope;
use alloy_primitives::{Address, Bytes};
use alloy_rpc_client::RpcClient;
use alloy_rpc_types_engine::JwtSecret;
use alloy_transport_http::{
    AuthLayer, Http, HyperClient,
    hyper_util::{client::legacy::Client, rt::TokioExecutor},
};
use http_body_util::Full;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower::ServiceBuilder;

use crate::constants::{TX_POOL_CONTENT_METHOD, TX_POOL_CONTENT_WITH_MIN_TIP_METHOD};

/// The [`TaikoAuthClient`] is responsible for interacting with the taikoAuth API via HTTP.
/// The inner transport uses a JWT [AuthLayer] to authenticate requests.
#[derive(Clone)]
pub struct TaikoAuthClient {
    inner: RpcClient,
}

impl TaikoAuthClient {
    /// Creates a new [`TaikoAuthClient`] from the provided [Url] and [JwtSecret].
    pub fn new(url: Url, jwt_secret: JwtSecret) -> Self {
        let hyper_client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();
        let auth_layer = AuthLayer::new(jwt_secret);
        let service = ServiceBuilder::new().layer(auth_layer).service(hyper_client);

        let layer_transport = HyperClient::with_service(service);
        let http_hyper = Http::with_client(layer_transport, url);

        Self { inner: RpcClient::new(http_hyper, true) }
    }

    #[allow(unused)]
    async fn fetch_mempool_txs(
        &self,
        beneficiary: Address,
        base_fee: u64,
        block_max_gas_limit: u64,
        max_bytes_per_tx_list: u64,
        locals: Vec<Address>,
        max_transactions_lists: u64,
    ) -> eyre::Result<Vec<Vec<TxEnvelope>>> {
        let params = vec![
            json!(beneficiary),
            json!(base_fee),
            json!(block_max_gas_limit),
            json!(max_bytes_per_tx_list),
            json!(locals),
            json!(max_transactions_lists),
        ];

        let rpc_call = self.inner.request(TX_POOL_CONTENT_METHOD, params);
        let response: Vec<PreBuiltTxList> = rpc_call.await?;

        Ok(response.into_iter().map(|prebuilt_tx_list| prebuilt_tx_list.tx_list).collect())
    }

    #[allow(unused)]
    #[allow(clippy::too_many_arguments)]
    async fn fetch_mempool_txs_with_min_tip(
        &self,
        beneficiary: Address,
        base_fee: u64,
        block_max_gas_limit: u64,
        max_bytes_per_tx_list: u64,
        locals: Vec<Address>,
        max_transactions_lists: u64,
        min_tip: u64,
    ) -> eyre::Result<Vec<Vec<TxEnvelope>>> {
        let params = vec![
            json!(beneficiary),
            json!(base_fee),
            json!(block_max_gas_limit),
            json!(max_bytes_per_tx_list),
            json!(locals),
            json!(max_transactions_lists),
            json!(min_tip),
        ];

        let rpc_call = self.inner.request(TX_POOL_CONTENT_WITH_MIN_TIP_METHOD, params);
        let response: Vec<PreBuiltTxList> = rpc_call.await?;

        Ok(response.into_iter().map(|prebuilt_tx_list| prebuilt_tx_list.tx_list).collect())
    }
}

/// PreBuiltTxList is a pre-built transaction list based on the latest chain state,
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PreBuiltTxList {
    tx_list: Vec<TxEnvelope>,
    estimated_gas_used: u64,
    bytes_length: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tx_pool_content_deserialisation() {
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
