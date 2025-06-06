use alloy_consensus::{Header, TxEnvelope};
use alloy_contract::Error as ContractError;
use alloy_json_rpc::RpcError;
use alloy_primitives::FixedBytes;
use alloy_primitives::{Address, ChainId, ruint::FromUintError};
use alloy_provider::Provider;
use alloy_provider::utils::Eip1559Estimation;
use alloy_rpc_types::{Header as RpcHeader, TransactionRequest};
use alloy_transport::TransportErrorKind;
use c_kzg::BYTES_PER_BLOB;
use k256::ecdsa::{Error as EcdsaError, SigningKey};
use libdeflater::CompressionError;
use thiserror::Error;
use tracing::debug;

use crate::client::{HttpError, RpcClient, flatten_mempool_txs, get_mempool_txs, get_nonce};
use crate::preconf::preconf_blocks::{
    BuildPreconfBlockRequest, BuildPreconfBlockResponse, create_executable_data,
};
use crate::taiko::{
    anchor::create_anchor_transaction,
    contracts::{Provider as TaikoProvider, TaikoAnchor, TaikoAnchorInstance},
    hekla::GAS_LIMIT,
    sign::get_signed,
};

#[derive(Debug, Error)]
pub enum TaikoL2ClientError {
    #[error("{0}")]
    Rpc(#[from] RpcError<TransportErrorKind>),

    #[error("{0}")]
    Http(#[from] HttpError),

    #[error("{0}")]
    Ecdsa(#[from] EcdsaError),

    #[error("{0}")]
    Contract(#[from] ContractError),

    #[error("{0}")]
    FromUInt128(#[from] FromUintError<u128>),

    #[error("{0}")]
    Compression(#[from] CompressionError),

    #[error("{0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("Reqwest error: code={code} error={error}")]
    InvalidReqwest { code: u16, error: String },
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

    fn get_nonce(&self, address: &str) -> impl Future<Output = TaikoL2ClientResult<u64>>;

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
    auth_client: RpcClient,
    taiko_anchor: TaikoAnchorInstance,
    provider: TaikoProvider,
    base_fee_config: TaikoAnchor::BaseFeeConfig,
    chain_id: ChainId,
    golden_touch_signing_key: SigningKey,
    preconfirmation_url: String,
}

impl TaikoL2Client {
    pub fn new(
        auth_client: RpcClient,
        taiko_anchor: TaikoAnchorInstance,
        provider: TaikoProvider,
        base_fee_config: TaikoAnchor::BaseFeeConfig,
        chain_id: ChainId,
        golden_touch_signing_key: SigningKey,
        preconfirmation_url: String,
    ) -> Self {
        Self {
            auth_client,
            taiko_anchor,
            provider,
            base_fee_config,
            chain_id,
            golden_touch_signing_key,
            preconfirmation_url,
        }
    }
}

impl ITaikoL2Client for TaikoL2Client {
    async fn get_mempool_txs(
        &self,
        beneficiary: Address,
        base_fee: u64,
    ) -> TaikoL2ClientResult<Vec<TxEnvelope>> {
        let mempool_tx_lists = get_mempool_txs(
            &self.auth_client,
            beneficiary,
            base_fee,
            GAS_LIMIT,
            BYTES_PER_BLOB as u64,
            vec![],
            1,
        )
        .await?;
        Ok(flatten_mempool_txs(mempool_tx_lists))
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

    async fn get_nonce(&self, address: &str) -> TaikoL2ClientResult<u64> {
        Ok(get_nonce(self.provider.client(), address).await?)
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
        debug!("executable data {executable_data:?}");
        let end_of_sequencing = false;

        let req = BuildPreconfBlockRequest {
            executable_data,
            end_of_sequencing,
        };
        let client = reqwest::Client::new();
        let request_builder = client.post(&self.preconfirmation_url);
        let response = request_builder.json(&req).send().await?;
        let status = response.status();
        if !status.is_success() {
            return Err(TaikoL2ClientError::InvalidReqwest {
                code: status.as_u16(),
                error: response.text().await?,
            });
            //            info!("{}", response.text().await?);
            //            let response = client.post(url).json(&req).send().await?;
            //            return Err(response.error_for_status().unwrap_err().into());
        }

        let response: BuildPreconfBlockResponse = response.json().await?;
        Ok(response.block_header)
    }
}
