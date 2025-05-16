use alloy_contract::Error as ContractError;
use alloy_json_rpc::RpcError;
use alloy_transport::TransportErrorKind;
use serde_json::Value as JsonValue;
use thiserror::Error;
use url::ParseError;

#[derive(Error, Debug)]
pub enum PreconferError {
    #[error("{0}")]
    Rpc(#[from] RpcError<TransportErrorKind>),

    #[error("{0}")]
    UrlParse(#[from] ParseError),

    #[error("{0}")]
    Contract(#[from] ContractError),

    #[error("RPC request {method} with {params} failed.")]
    FailedRPCRequest { method: String, params: JsonValue },
}

pub type PreconferResult<T> = Result<T, PreconferError>;
