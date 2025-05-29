use alloy_consensus::Header;
use thiserror::Error;

use crate::{
    http_client::{get_header_by_id, get_nonce},
    rpc_client::RpcClient,
};

#[derive(Debug, Error)]
pub enum TaikoL1ClientError {
    #[error("{0}")]
    Rpc(#[from] alloy_json_rpc::RpcError<alloy_transport::TransportErrorKind>),

    #[error("{0}")]
    Http(#[from] crate::http_client::HttpError),

    #[error("{0}")]
    Contract(#[from] alloy_contract::Error),

    #[error("{0}")]
    FromUInt128(#[from] alloy_primitives::ruint::FromUintError<u128>),
}

pub type TaikoL1ClientResult<T> = Result<T, TaikoL1ClientError>;

#[cfg_attr(test, mockall::automock)]
pub trait ITaikoL1Client {
    fn get_nonce(&self, address: &str) -> impl Future<Output = TaikoL1ClientResult<u64>>;

    fn get_header(&self, id: u64) -> impl Future<Output = TaikoL1ClientResult<Header>>;
}

pub struct TaikoL1Client {
    client: RpcClient,
}

impl TaikoL1Client {
    pub const fn new(client: RpcClient) -> Self {
        Self { client }
    }
}

impl ITaikoL1Client for TaikoL1Client {
    async fn get_nonce(&self, address: &str) -> TaikoL1ClientResult<u64> {
        Ok(get_nonce(&self.client, address).await?)
    }

    async fn get_header(&self, id: u64) -> TaikoL1ClientResult<Header> {
        Ok(get_header_by_id(&self.client, id).await?)
    }
}
