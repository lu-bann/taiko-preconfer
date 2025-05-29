use alloy_json_rpc::RpcError;
use alloy_transport::TransportErrorKind;
use thiserror::Error;
use url::ParseError;

#[derive(Error, Debug)]
pub enum ApplicationError {
    #[error("{0}")]
    Rpc(#[from] RpcError<TransportErrorKind>),

    #[error("{0}")]
    UrlParse(#[from] ParseError),

    #[error("{0}")]
    Preconfer(#[from] block_building::preconf::PreconferError),

    #[error("Web socket connection lost at {url}.")]
    WsConnectionLost { url: String },
}

pub type ApplicationResult<T> = Result<T, ApplicationError>;
