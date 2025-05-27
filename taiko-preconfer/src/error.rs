use alloy_contract::Error as ContractError;
use alloy_json_rpc::RpcError;
use alloy_transport::TransportErrorKind;
use block_building::http_client::HttpError;
use k256::ecdsa::Error as EcdsaError;
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

    #[error("{0}")]
    Ecdsa(#[from] EcdsaError),

    #[error("{0}")]
    Http(#[from] HttpError),

    #[error("RPC request {method} with {params} failed.")]
    FailedRPCRequest { method: String, params: JsonValue },
}

pub type PreconferResult<T> = Result<T, PreconferError>;
