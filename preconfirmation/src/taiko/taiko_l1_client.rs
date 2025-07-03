use std::thread::JoinHandle;

use alloy_consensus::Header;
use alloy_eips::BlockNumberOrTag;
use alloy_network::{EthereumWallet, TransactionBuilder};
use alloy_primitives::Address;
use alloy_provider::{
    Identity, Provider, RootProvider,
    fillers::{
        BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller, WalletFiller,
    },
    utils::Eip1559Estimation,
};
use alloy_rpc_types::TransactionRequest;
use libdeflater::CompressionError;
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};
use thiserror::Error;
use tokio::{join, sync::RwLock};
use tracing::{error, info};

use crate::{blob::BlobEncodeError, util::log_error, verification::TaikoInboxError};

pub type TaikoProvider = FillProvider<
    JoinFill<
        JoinFill<
            Identity,
            JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>,
        >,
        WalletFiller<EthereumWallet>,
    >,
    RootProvider,
>;

#[derive(Debug, Error)]
pub enum TaikoL1ClientError {
    #[error("{0}")]
    Rpc(String),

    #[error("{0}")]
    BlobEncode(#[from] BlobEncodeError),

    #[error("{0}")]
    Compression(#[from] CompressionError),

    #[error("{0}")]
    TaikoInbox(#[from] TaikoInboxError),

    #[error("{0}")]
    Contract(#[from] alloy_contract::Error),

    #[error("{0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("{0}")]
    IO(#[from] std::io::Error),

    #[error("{0}")]
    FromUInt128(#[from] alloy_primitives::ruint::FromUintError<u128>),

    #[error("{0}")]
    PendingTransaction(#[from] alloy_provider::PendingTransactionError),
}

impl From<alloy_json_rpc::RpcError<alloy_transport::TransportErrorKind>> for TaikoL1ClientError {
    fn from(err: alloy_json_rpc::RpcError<alloy_transport::TransportErrorKind>) -> Self {
        Self::Rpc(crate::util::parse_transport_error(err))
    }
}

pub type TaikoL1ClientResult<T> = Result<T, TaikoL1ClientError>;

#[cfg_attr(test, mockall::automock)]
pub trait ITaikoL1Client {
    fn get_nonce(&self, address: Address) -> impl Future<Output = TaikoL1ClientResult<u64>>;

    fn get_blob_base_fee(&self) -> impl Future<Output = TaikoL1ClientResult<u128>>;

    fn get_header(&self, id: u64) -> impl Future<Output = TaikoL1ClientResult<Header>>;

    fn get_latest_header(&self) -> impl Future<Output = TaikoL1ClientResult<Header>>;

    fn estimate_gas(
        &self,
        tx: TransactionRequest,
    ) -> impl Future<Output = TaikoL1ClientResult<u64>>;

    fn estimate_eip1559_fees(&self)
    -> impl Future<Output = TaikoL1ClientResult<Eip1559Estimation>>;

    fn send(&self, tx: TransactionRequest) -> impl Future<Output = TaikoL1ClientResult<()>>;
}

#[derive(Debug, Clone)]
pub struct TaikoL1Client {
    provider: TaikoProvider,
    propose_timeout: Duration,
    tx_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
    preconfer_address: Address,
    taiko_inbox: Address,
}

impl TaikoL1Client {
    pub fn new(
        provider: TaikoProvider,
        propose_timeout: Duration,
        preconfer_address: Address,
        taiko_inbox: Address,
    ) -> Self {
        Self {
            provider,
            propose_timeout,
            tx_handle: Arc::new(None.into()),
            preconfer_address,
            taiko_inbox,
        }
    }
}

impl ITaikoL1Client for TaikoL1Client {
    async fn get_nonce(&self, address: Address) -> TaikoL1ClientResult<u64> {
        Ok(self.provider.get_transaction_count(address).await?)
    }

    async fn get_blob_base_fee(&self) -> TaikoL1ClientResult<u128> {
        Ok(self.provider.get_blob_base_fee().await?)
    }

    async fn get_header(&self, id: u64) -> TaikoL1ClientResult<Header> {
        Ok(self
            .provider
            .get_block_by_number(BlockNumberOrTag::Number(id))
            .await?
            .ok_or(alloy_json_rpc::RpcError::NullResp)?
            .header
            .inner)
    }

    async fn get_latest_header(&self) -> TaikoL1ClientResult<Header> {
        Ok(self
            .provider
            .get_block_by_number(BlockNumberOrTag::Latest)
            .await?
            .ok_or(alloy_json_rpc::RpcError::NullResp)?
            .header
            .inner)
    }

    async fn estimate_gas(&self, tx: TransactionRequest) -> TaikoL1ClientResult<u64> {
        Ok(self.provider.estimate_gas(tx).await?)
    }

    async fn estimate_eip1559_fees(&self) -> TaikoL1ClientResult<Eip1559Estimation> {
        Ok(self.provider.estimate_eip1559_fees().await?)
    }

    async fn send(&self, tx: TransactionRequest) -> TaikoL1ClientResult<()> {
        info!("Send tx");
        let previous_tx_returned = self
            .tx_handle
            .read()
            .await
            .as_ref()
            .map(|handle| handle.is_finished())
            .unwrap_or(true);
        if !previous_tx_returned {
            info!("Previous transaction did not finish. Skipping tx.");
            return Ok(());
        }
        let provider = self.provider.clone();
        let timeout = self.propose_timeout;
        let preconfer_address = self.preconfer_address;
        let taiko_inbox_address = self.taiko_inbox;
        *self.tx_handle.write().await = Some(
            std::thread::Builder::new()
                .stack_size(8 * 1024 * 1024)
                .spawn(move || {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    rt.block_on(async move {
                        let start = SystemTime::now();

                        let mut tx = tx;
                        if tx.sidecar.is_some() {
                            info!("Set blob base fee");
                            if let Some(blob_base_fee) = log_error(
                                provider.get_blob_base_fee().await,
                                "Failed to get blob base fee",
                            ) {
                                tx = tx.max_fee_per_blob_gas(blob_base_fee);
                            }
                        }
                        info!("tx: {:?}", tx);
                        let (nonce, gas_limit, fee_estimate) = join!(
                            provider.get_transaction_count(preconfer_address),
                            provider.estimate_gas(tx.clone()),
                            provider.estimate_eip1559_fees(),
                        );

                        info!(
                            "sign tx {} {:?} {:?} {:?}",
                            taiko_inbox_address, nonce, gas_limit, fee_estimate
                        );
                        if gas_limit.is_err() || fee_estimate.is_err() {
                            error!("Failed to estimate gas or fees for block confirmation.");
                            error!("{}", gas_limit.unwrap_err());
                            error!("{}", fee_estimate.unwrap_err());
                            return;
                        }
                        let gas_limit = gas_limit.expect("Must be present");
                        let fee_estimate = fee_estimate.expect("Must be present");
                        let nonce = nonce.expect("Must be present");
                        let tx = tx
                            .with_gas_limit(gas_limit)
                            .with_max_fee_per_gas(fee_estimate.max_fee_per_gas * 12 / 10)
                            .with_max_priority_fee_per_gas(
                                fee_estimate.max_priority_fee_per_gas * 12 / 10,
                            )
                            .nonce(nonce);

                        info!("propose batch tx {tx:?}");

                        if let Some(tx_builder) = log_error(
                            provider.send_transaction(tx).await,
                            "Failed to get transaction builder",
                        ) {
                            if let Some(receipt) = log_error(
                                tx_builder
                                    .with_required_confirmations(2)
                                    .with_timeout(Some(timeout))
                                    .get_receipt()
                                    .await,
                                "Failed to send transaction",
                            ) {
                                let end = SystemTime::now();
                                let elapsed = end.duration_since(start).unwrap().as_millis();
                                info!("receipt: {receipt:?}, elapsed={elapsed} ms");
                            }
                        }
                    });
                })?,
        );
        Ok(())
    }
}
