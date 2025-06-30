use alloy_consensus::{Header, TxEnvelope};
use alloy_contract::Error as ContractError;
use alloy_json_rpc::RpcError;
use alloy_primitives::{Address, Bytes, ChainId, FixedBytes, ruint::FromUintError};
use alloy_provider::Provider;
use alloy_provider::utils::Eip1559Estimation;
use alloy_rlp::RlpEncodable;
use alloy_rpc_types::{Header as RpcHeader, TransactionRequest};
use alloy_rpc_types_engine::Claims;
use alloy_transport::TransportErrorKind;
use k256::ecdsa::{Error as EcdsaError, SigningKey};
use libdeflater::CompressionError;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::info;

use crate::blob::MAX_BLOB_DATA_SIZE;
use crate::{compression::compress, util::pad_left};

use crate::reqwest::{auth_reqwest, json_auth_reqwest};
use crate::secret::Secret;
use crate::taiko::{
    anchor::create_anchor_transaction,
    contracts::{BaseFeeConfig, Provider as TaikoProvider, TaikoAnchor, TaikoAnchorInstance},
    sign::get_signed,
};
use crate::util::{get_latest_header, hex_decode, now_as_secs};

const GAS_LIMIT: u64 = 241_000_000;
const TAIKO_TX_POOL_CONTENT: &str = "taikoAuth_txPoolContent";
#[allow(dead_code)]
const TAIKO_TX_POOL_CONTENT_WITH_MIN_TIP: &str = "taikoAuth_txPoolContentWithMinTip";

#[derive(Debug, Error)]
pub enum TaikoL2ClientError {
    #[error("{0}")]
    Rpc(#[from] RpcError<TransportErrorKind>),

    #[error("{0}")]
    RpcReqwest(#[from] crate::reqwest::RpcError),

    #[error("{0}")]
    FromHex(#[from] hex::FromHexError),

    #[error("{0}")]
    UrlParse(#[from] url::ParseError),

    #[error("{0}")]
    Ecdsa(#[from] EcdsaError),

    #[error("{0}")]
    Contract(#[from] ContractError),

    #[error("{0}")]
    FromUInt128(#[from] FromUintError<u128>),

    #[error("{0}")]
    Compression(#[from] CompressionError),

    #[error("{0}")]
    JsonWebToken(#[from] jsonwebtoken::errors::Error),
}

pub type TaikoL2ClientResult<T> = Result<T, TaikoL2ClientError>;

#[cfg_attr(test, mockall::automock)]
pub trait ITaikoL2Client {
    fn get_mempool_txs(
        &self,
        beneficiary: Address,
        base_fee: u64,
    ) -> impl Future<Output = TaikoL2ClientResult<Vec<TxEnvelope>>>;

    fn get_base_fee(
        &self,
        parent_gas_used: u64,
        timestamp: u64,
    ) -> impl Future<Output = TaikoL2ClientResult<u128>>;

    fn get_nonce(&self, address: Address) -> impl Future<Output = TaikoL2ClientResult<u64>>;

    fn get_latest_header(&self) -> impl Future<Output = TaikoL2ClientResult<Header>>;

    fn estimate_gas(
        &self,
        tx: TransactionRequest,
    ) -> impl Future<Output = TaikoL2ClientResult<u64>>;

    fn estimate_eip1559_fees(&self)
    -> impl Future<Output = TaikoL2ClientResult<Eip1559Estimation>>;

    fn get_signed_anchor_tx(
        &self,
        anchor_block_id: u64,
        anchor_state_root: FixedBytes<32>,
        parent_gas_used: u32,
        nonce: u64,
        max_fee_per_gas: u128,
    ) -> TaikoL2ClientResult<TxEnvelope>;

    fn publish_preconfirmed_transactions(
        &self,
        fee_recipient: Address,
        base_fee: u64,
        timestamp: u64,
        parent_header: &Header,
        txs: Vec<TxEnvelope>,
    ) -> impl Future<Output = TaikoL2ClientResult<RpcHeader>>;
}

#[derive(Debug)]
pub struct TaikoL2Client {
    auth_url: String,
    taiko_anchor: TaikoAnchorInstance,
    provider: TaikoProvider,
    base_fee_config: BaseFeeConfig,
    chain_id: ChainId,
    golden_touch_signing_key: SigningKey,
    preconfirmation_url: String,
    jwt_secret: Secret,
}

impl TaikoL2Client {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        auth_url: String,
        taiko_anchor: TaikoAnchorInstance,
        provider: TaikoProvider,
        base_fee_config: BaseFeeConfig,
        chain_id: ChainId,
        golden_touch_signing_key: SigningKey,
        preconfirmation_url: String,
        jwt_secret: Secret,
    ) -> Self {
        Self {
            auth_url,
            taiko_anchor,
            provider,
            base_fee_config,
            chain_id,
            golden_touch_signing_key,
            preconfirmation_url,
            jwt_secret,
        }
    }
}

impl ITaikoL2Client for TaikoL2Client {
    async fn get_mempool_txs(
        &self,
        beneficiary: Address,
        base_fee: u64,
    ) -> TaikoL2ClientResult<Vec<TxEnvelope>> {
        let params = serde_json::json!([
            beneficiary,
            base_fee,
            GAS_LIMIT,
            MAX_BLOB_DATA_SIZE,
            Vec::<u64>::default(),
            1u8
        ]);
        let jwt_secret = self.get_jwt_secret()?;

        if let Some(mempool_tx_lists) = json_auth_reqwest(
            &self.auth_url,
            TAIKO_TX_POOL_CONTENT.into(),
            params,
            jwt_secret,
        )
        .await?
        {
            Ok(flatten_mempool_txs(mempool_tx_lists))
        } else {
            Ok(vec![])
        }
    }

    async fn get_base_fee(
        &self,
        parent_gas_used: u64,
        timestamp: u64,
    ) -> TaikoL2ClientResult<u128> {
        let base_fee = self
            .taiko_anchor
            .getBasefeeV2(
                parent_gas_used as u32,
                timestamp,
                self.base_fee_config.clone(),
            )
            .call()
            .await?;
        Ok(base_fee.basefee_.try_into()?)
    }

    async fn get_nonce(&self, address: Address) -> TaikoL2ClientResult<u64> {
        Ok(self.provider.get_transaction_count(address).await?)
    }

    async fn get_latest_header(&self) -> TaikoL2ClientResult<Header> {
        Ok(get_latest_header(self.provider.client()).await?)
    }

    async fn estimate_gas(&self, tx: TransactionRequest) -> TaikoL2ClientResult<u64> {
        Ok(self.provider.estimate_gas(tx).await?)
    }

    async fn estimate_eip1559_fees(&self) -> TaikoL2ClientResult<Eip1559Estimation> {
        Ok(self.provider.estimate_eip1559_fees().await?)
    }

    fn get_signed_anchor_tx(
        &self,
        anchor_block_id: u64,
        anchor_state_root: FixedBytes<32>,
        parent_gas_used: u32,
        nonce: u64,
        max_fee_per_gas: u128,
    ) -> TaikoL2ClientResult<TxEnvelope> {
        let anchor_call = TaikoAnchor::anchorV3Call {
            _anchorBlockId: anchor_block_id,
            _anchorStateRoot: anchor_state_root,
            _parentGasUsed: parent_gas_used,
            _baseFeeConfig: self.base_fee_config.clone(),
            _signalSlots: vec![],
        };
        let anchor_tx = create_anchor_transaction(
            self.chain_id,
            nonce,
            max_fee_per_gas,
            0u128,
            *self.taiko_anchor.address(),
            anchor_call,
        );

        let signed_anchor_tx = get_signed(&self.golden_touch_signing_key, anchor_tx.clone())?;
        Ok(TxEnvelope::from(signed_anchor_tx))
    }

    async fn publish_preconfirmed_transactions(
        &self,
        fee_recipient: Address,
        base_fee: u64,
        timestamp: u64,
        parent_header: &Header,
        txs: Vec<TxEnvelope>,
    ) -> TaikoL2ClientResult<RpcHeader> {
        let executable_data = create_executable_data(
            base_fee,
            parent_header.number + 1,
            self.base_fee_config.sharingPctg,
            fee_recipient,
            GAS_LIMIT,
            parent_header.hash_slow(),
            timestamp,
            txs,
        )?;
        info!("executable data {executable_data:?}");
        let end_of_sequencing = false;

        let request = BuildPreconfBlockRequest {
            executable_data,
            end_of_sequencing,
        };

        let jwt_secret = self.get_jwt_secret()?;
        let response: BuildPreconfBlockResponse =
            auth_reqwest(&self.preconfirmation_url, &request, jwt_secret).await?;
        Ok(response.block_header)
    }
}

impl TaikoL2Client {
    pub fn get_jwt_secret(&self) -> TaikoL2ClientResult<String> {
        let secret_bytes = hex_decode(self.jwt_secret.read_slice())?;
        let now = now_as_secs();
        let claims = Claims {
            iat: now,
            exp: Some(now + 3600),
        };
        let jwt_token = jsonwebtoken::encode(
            &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
            &claims,
            &jsonwebtoken::EncodingKey::from_secret(&secret_bytes),
        )?;
        Ok(format!("Bearer {jwt_token}"))
    }
}

#[derive(Debug, Serialize, RlpEncodable)]
#[serde(rename_all = "camelCase")]
pub struct ExecutableData {
    parent_hash: FixedBytes<32>,
    fee_recipient: Address,
    block_number: u64,
    gas_limit: u64,
    timestamp: u64,
    transactions: Bytes,
    extra_data: Bytes,
    base_fee_per_gas: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildPreconfBlockRequest {
    pub executable_data: ExecutableData,
    pub end_of_sequencing: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildPreconfBlockResponse {
    pub block_header: RpcHeader,
}

#[allow(clippy::too_many_arguments)]
pub fn create_executable_data(
    base_fee_per_gas: u64,
    block_number: u64,
    sharing_percentage: u8,
    fee_recipient: Address,
    gas_limit: u64,
    parent_hash: FixedBytes<32>,
    timestamp: u64,
    txs: Vec<TxEnvelope>,
) -> Result<ExecutableData, CompressionError> {
    info!(
        "tx hash {}",
        alloy_primitives::keccak256(alloy_rlp::encode(&txs))
    );
    let tx_bytes = compress(txs)?;
    let transactions = Bytes::from(tx_bytes);

    Ok(ExecutableData {
        base_fee_per_gas,
        block_number,
        extra_data: pad_left::<32>(&[sharing_percentage]),
        fee_recipient,
        gas_limit,
        parent_hash,
        timestamp,
        transactions,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct MempoolTxList {
    pub tx_list: Vec<TxEnvelope>,
    #[allow(dead_code)]
    pub estimated_gas_used: u64,
    #[allow(dead_code)]
    pub bytes_length: u64,
}

fn flatten_mempool_txs(tx_lists: Vec<MempoolTxList>) -> Vec<TxEnvelope> {
    tx_lists
        .into_iter()
        .flat_map(|tx_list| tx_list.tx_list)
        .collect()
}
