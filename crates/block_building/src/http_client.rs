use std::future::Future;

use alloy_json_rpc::{RpcError, RpcRecv};
use alloy_transport::TransportErrorKind;
#[cfg(test)]
use mockall::automock;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum HttpError {
    #[error("{0}")]
    Rpc(#[from] RpcError<TransportErrorKind>),
}

#[cfg_attr(test, automock)]
pub trait HttpClient {
    fn get<Resp: RpcRecv>(
        &self,
        method: String,
        params: serde_json::Value,
    ) -> impl Future<Output = Result<Resp, HttpError>>;
}
