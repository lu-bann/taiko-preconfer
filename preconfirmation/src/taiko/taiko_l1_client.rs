use alloy_consensus::Header;
use alloy_network::EthereumWallet;
use alloy_primitives::ChainId;
use alloy_provider::{
    Identity, Provider, RootProvider,
    fillers::{
        BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller, WalletFiller,
    },
    utils::Eip1559Estimation,
};
use alloy_rpc_types::TransactionRequest;
use std::time::SystemTime;
use thiserror::Error;
use tracing::info;

use crate::client::{get_header_by_id, get_latest_header, get_nonce};

type TaikoProvider = FillProvider<
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
    Rpc(#[from] alloy_json_rpc::RpcError<alloy_transport::TransportErrorKind>),

    #[error("{0}")]
    Http(#[from] crate::client::HttpError),

    #[error("{0}")]
    Contract(#[from] alloy_contract::Error),

    #[error("{0}")]
    FromUInt128(#[from] alloy_primitives::ruint::FromUintError<u128>),

    #[error("{0}")]
    PendingTransaction(#[from] alloy_provider::PendingTransactionError),
}

pub type TaikoL1ClientResult<T> = Result<T, TaikoL1ClientError>;

#[cfg_attr(test, mockall::automock)]
pub trait ITaikoL1Client {
    fn get_nonce(&self, address: &str) -> impl Future<Output = TaikoL1ClientResult<u64>>;

    fn get_header(&self, id: u64) -> impl Future<Output = TaikoL1ClientResult<Header>>;

    fn get_latest_header(&self) -> impl Future<Output = TaikoL1ClientResult<Header>>;

    fn estimate_gas(
        &self,
        tx: TransactionRequest,
    ) -> impl Future<Output = TaikoL1ClientResult<u64>>;

    fn estimate_eip1559_fees(&self)
    -> impl Future<Output = TaikoL1ClientResult<Eip1559Estimation>>;

    fn chain_id(&self) -> ChainId;

    fn send(&self, tx: TransactionRequest) -> impl Future<Output = TaikoL1ClientResult<()>>;
}

#[derive(Debug, Clone)]
pub struct TaikoL1Client {
    provider: TaikoProvider,
    chain_id: ChainId,
}

impl TaikoL1Client {
    pub const fn new(provider: TaikoProvider, chain_id: ChainId) -> Self {
        Self { provider, chain_id }
    }
}

impl ITaikoL1Client for TaikoL1Client {
    async fn get_nonce(&self, address: &str) -> TaikoL1ClientResult<u64> {
        Ok(get_nonce(self.provider.client(), address).await?)
    }

    async fn get_header(&self, id: u64) -> TaikoL1ClientResult<Header> {
        Ok(get_header_by_id(self.provider.client(), id).await?)
    }

    async fn get_latest_header(&self) -> TaikoL1ClientResult<Header> {
        Ok(get_latest_header(self.provider.client()).await?)
    }

    async fn estimate_gas(&self, tx: TransactionRequest) -> TaikoL1ClientResult<u64> {
        Ok(self.provider.estimate_gas(tx).await?)
    }

    async fn estimate_eip1559_fees(&self) -> TaikoL1ClientResult<Eip1559Estimation> {
        Ok(self.provider.estimate_eip1559_fees().await?)
    }

    fn chain_id(&self) -> ChainId {
        self.chain_id
    }

    async fn send(&self, tx: TransactionRequest) -> TaikoL1ClientResult<()> {
        info!("Send tx");
        let start = SystemTime::now();
        let receipt = self
            .provider
            .send_transaction(tx)
            .await?
            .with_required_confirmations(2)
            .with_timeout(Some(std::time::Duration::from_secs(60)))
            .get_receipt()
            .await?;
        let end = SystemTime::now();
        let elapsed = end.duration_since(start).unwrap().as_millis();
        info!("receipt: {receipt:?}, elapsed={elapsed} ms");
        Ok(())
    }
}
