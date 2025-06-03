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
use k256::ecdsa::Error as EcdsaError;
use libdeflater::CompressionError;
use thiserror::Error;
use tracing::debug;

use crate::client::{
    DummyClient, HttpError, RpcClient, flatten_mempool_txs, get_mempool_txs, get_nonce,
};
use crate::preconf::preconf_blocks::{create_executable_data, publish_preconfirmed_transactions};
use crate::taiko::{
    anchor::create_anchor_transaction,
    contracts::{Provider as TaikoProvider, TaikoAnchor, TaikoAnchorInstance},
    hekla::GAS_LIMIT,
    sign::get_signed_with_golden_touch,
};

#[derive(Debug, Error)]
pub enum TaikoClientError {
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
}

pub type TaikoClientResult<T> = Result<T, TaikoClientError>;

#[cfg_attr(test, mockall::automock)]
pub trait ITaikoClient {
    fn get_mempool_txs(
        &self,
        beneficiary: Address,
        base_fee: u64,
    ) -> impl Future<Output = TaikoClientResult<Vec<TxEnvelope>>>;

    fn get_base_fee(
        &self,
        parent_gas_used: u64,
        timestamp: u64,
    ) -> impl Future<Output = TaikoClientResult<u128>>;

    fn get_nonce(&self, address: &str) -> impl Future<Output = TaikoClientResult<u64>>;

    fn estimate_gas(&self, tx: TransactionRequest) -> impl Future<Output = TaikoClientResult<u64>>;

    fn estimate_eip1559_fees(&self) -> impl Future<Output = TaikoClientResult<Eip1559Estimation>>;

    fn get_signed_anchor_tx(
        &self,
        anchor_block_id: u64,
        anchor_state_root: FixedBytes<32>,
        parent_gas_used: u32,
        nonce: u64,
        max_fee_per_gas: u128,
    ) -> TaikoClientResult<TxEnvelope>;

    fn publish_preconfirmed_transactions(
        &self,
        fee_recipient: Address,
        base_fee: u64,
        timestamp: u64,
        parent_header: &Header,
        txs: Vec<TxEnvelope>,
    ) -> impl Future<Output = TaikoClientResult<RpcHeader>>;
}

#[derive(Debug)]
pub struct TaikoClient {
    client: RpcClient,
    auth_client: RpcClient,
    taiko_anchor: TaikoAnchorInstance,
    provider: TaikoProvider,
    base_fee_config: TaikoAnchor::BaseFeeConfig,
    chain_id: ChainId,
}

impl TaikoClient {
    pub fn new(
        client: RpcClient,
        auth_client: RpcClient,
        taiko_anchor: TaikoAnchorInstance,
        provider: TaikoProvider,
        base_fee_config: TaikoAnchor::BaseFeeConfig,
        chain_id: ChainId,
    ) -> Self {
        Self {
            client,
            auth_client,
            taiko_anchor,
            provider,
            base_fee_config,
            chain_id,
        }
    }
}

impl ITaikoClient for TaikoClient {
    async fn get_mempool_txs(
        &self,
        beneficiary: Address,
        base_fee: u64,
    ) -> TaikoClientResult<Vec<TxEnvelope>> {
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

    async fn get_base_fee(&self, parent_gas_used: u64, timestamp: u64) -> TaikoClientResult<u128> {
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

    async fn get_nonce(&self, address: &str) -> TaikoClientResult<u64> {
        Ok(get_nonce(&self.client, address).await?)
    }

    async fn estimate_gas(&self, tx: TransactionRequest) -> TaikoClientResult<u64> {
        Ok(self.provider.estimate_gas(tx).await?)
    }

    async fn estimate_eip1559_fees(&self) -> TaikoClientResult<Eip1559Estimation> {
        Ok(self.provider.estimate_eip1559_fees().await?)
    }

    fn get_signed_anchor_tx(
        &self,
        anchor_block_id: u64,
        anchor_state_root: FixedBytes<32>,
        parent_gas_used: u32,
        nonce: u64,
        max_fee_per_gas: u128,
    ) -> TaikoClientResult<TxEnvelope> {
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

        let signed_anchor_tx = get_signed_with_golden_touch(anchor_tx.clone())?;
        Ok(TxEnvelope::from(signed_anchor_tx))
    }

    async fn publish_preconfirmed_transactions(
        &self,
        fee_recipient: Address,
        base_fee: u64,
        timestamp: u64,
        parent_header: &Header,
        txs: Vec<TxEnvelope>,
    ) -> TaikoClientResult<RpcHeader> {
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
        let dummy_client = DummyClient {};
        let end_of_sequencing = false;
        let dummy_header =
            publish_preconfirmed_transactions(&dummy_client, executable_data, end_of_sequencing)
                .await?;
        debug!("header {dummy_header:?}");
        Ok(dummy_header)
    }
}
