use std::{collections::HashMap, str::FromStr};

use alloy_consensus::{Signed, TxEip1559, TxEip2930, TxEnvelope, TxLegacy};
use alloy_eips::eip2930::AccessList;
use alloy_primitives::{Address, B256, Bytes, ChainId, U256, hex};
use alloy_rpc_client::RpcClient;
use alloy_rpc_types_engine::JwtSecret;
use alloy_transport_http::{
    AuthLayer, Http, HyperClient,
    hyper_util::{client::legacy::Client, rt::TokioExecutor},
};
use async_trait::async_trait;
use http_body_util::Full;
use reqwest::Url;
use serde::{Deserialize, Deserializer, Serialize};
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

        Ok(response
            .into_iter()
            .flat_map(|prebuilt_tx_list| prebuilt_tx_list.into_tx_lists())
            .collect())
    }
}

/// PreBuiltTxList is a pre-built transaction list based on the latest chain state,
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PreBuiltTxList {
    pub tx_list: Vec<TaikoTx>,
    pub estimated_gas_used: u64,
    pub bytes_length: u64,
}

impl PreBuiltTxList {
    pub fn into_tx_lists(self) -> Vec<TxList> {
        let mut signer_map: HashMap<Address, Vec<TxEnvelope>> = HashMap::new();

        for (i, tx) in self.tx_list.into_iter().enumerate() {
            let envelope = to_alloy_tx(&tx);
            let signer = envelope
                .clone()
                .into_signed()
                .recover_signer()
                .unwrap_or_else(|_| panic!("Failed to recover signer from tx at index {}", i));

            signer_map.entry(signer).or_default().push(envelope);
        }

        signer_map.into_iter().map(|(account, txs)| TxList { account, txs: txs.into() }).collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaikoTx {
    #[serde(rename = "type")]
    pub tx_type: String,
    #[serde(deserialize_with = "hex_to_u64")]
    pub chain_id: u64,
    #[serde(deserialize_with = "hex_to_u64")]
    pub nonce: u64,
    pub to: String,
    #[serde(deserialize_with = "hex_to_u64")]
    pub gas: u64,
    #[serde(deserialize_with = "hex_to_u128_opt")]
    pub gas_price: Option<u128>,
    #[serde(deserialize_with = "hex_to_u128_opt")]
    pub max_priority_fee_per_gas: Option<u128>,
    #[serde(deserialize_with = "hex_to_u128_opt")]
    pub max_fee_per_gas: Option<u128>,
    pub value: U256,
    pub input: String,
    #[serde(default)]
    pub access_list: Vec<String>,
    pub v: String,
    pub r: U256,
    pub s: U256,
    #[serde(default, deserialize_with = "hex_str_to_bool")]
    pub y_parity: bool,
    pub hash: B256,
}

/// Parameters for the `taikoAuth_txPoolContent` & `taikoAuth_txPoolContentWithMinTip` RPC methods.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

fn to_alloy_tx(tx: &TaikoTx) -> TxEnvelope {
    match tx.tx_type.as_str() {
        "0x0" => {
            let transaction = TxLegacy {
                chain_id: Some(tx.chain_id),
                nonce: tx.nonce,
                to: alloy_primitives::TxKind::Call(Address::from_str(&tx.to).unwrap()),
                gas_limit: tx.gas,
                gas_price: tx.gas_price.unwrap(),
                value: tx.value,
                input: hex::decode(tx.input.clone()).unwrap().into(),
            };
            let signature = alloy_primitives::Signature::new(tx.r, tx.s, tx.y_parity);
            let signed_tx = Signed::new_unchecked(transaction, signature, tx.hash);
            TxEnvelope::Legacy(signed_tx)
        }
        "0x1" => {
            let transaction = TxEip2930 {
                chain_id: ChainId::from(tx.chain_id),
                nonce: tx.nonce,
                gas_limit: tx.gas,
                gas_price: tx.gas_price.unwrap(),
                to: alloy_primitives::TxKind::Call(Address::from_str(&tx.to).unwrap()),
                value: tx.value,
                access_list: AccessList::default(),
                input: hex::decode(tx.input.clone()).unwrap().into(),
            };
            let signature = alloy_primitives::Signature::new(tx.r, tx.s, tx.y_parity);
            let signed_tx = Signed::new_unchecked(transaction, signature, tx.hash);
            TxEnvelope::Eip2930(signed_tx)
        }
        "0x2" => {
            let transaction = TxEip1559 {
                chain_id: ChainId::from(tx.chain_id),
                nonce: tx.nonce,
                gas_limit: tx.gas,
                max_fee_per_gas: tx.max_fee_per_gas.unwrap(),
                max_priority_fee_per_gas: tx.max_priority_fee_per_gas.unwrap(),
                to: alloy_primitives::TxKind::Call(Address::from_str(&tx.to).unwrap()),
                value: tx.value,
                access_list: AccessList::default(),
                input: hex::decode(tx.input.clone()).unwrap().into(),
            };
            let signature = alloy_primitives::Signature::new(tx.r, tx.s, tx.y_parity);
            let signed_tx = Signed::new_unchecked(transaction, signature, tx.hash);
            TxEnvelope::Eip1559(signed_tx)
        }
        _ => todo!(),
    }
}

fn hex_to_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let hex_str: String = Deserialize::deserialize(deserializer)?;
    let hex = hex_str.strip_prefix("0x").unwrap_or(&hex_str);
    u64::from_str_radix(hex, 16).map_err(serde::de::Error::custom)
}

fn hex_to_u128_opt<'de, D>(deserializer: D) -> Result<Option<u128>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(deserializer)?;
    match opt {
        Some(s) => {
            let hex = s.strip_prefix("0x").unwrap_or(&s);
            u128::from_str_radix(hex, 16).map(Some).map_err(serde::de::Error::custom)
        }
        None => Ok(None),
    }
}

fn hex_str_to_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    match s.strip_prefix("0x").unwrap_or(&s) {
        "0" => Ok(false),
        "1" => Ok(true),
        other => Err(serde::de::Error::custom(format!("Invalid y_parity value: {other}"))),
    }
}

#[cfg(test)]
mod tests {

    use serde::de::{IntoDeserializer, value::Error as DeError};

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

        let mempool_txs: Vec<TxList> = rpc_response
            .result
            .into_iter()
            .flat_map(|prebuilt_tx_list| prebuilt_tx_list.into_tx_lists())
            .collect();
        let elapsed = instant.elapsed().as_millis();
        println!("Mempool Transactions: {:?}", mempool_txs);
        println!("Elapsed time: {:?}ms", elapsed);
        let total_txs: usize = mempool_txs.iter().map(|tx_list| tx_list.txs.len()).sum();
        println!("Total transactions: {}", total_txs);
        Ok(())
    }

    #[test]
    fn test_hex_to_u64_valid() {
        let value: serde::de::value::StrDeserializer<DeError> = "0x1f".into_deserializer();
        let result = hex_to_u64(value).unwrap();
        assert_eq!(result, 31);
    }

    #[test]
    fn test_hex_to_u64_invalid() {
        let value: serde::de::value::StrDeserializer<DeError> = "not-a-hex".into_deserializer();
        let result = hex_to_u64(value);
        assert!(result.is_err());
    }
}
