use std::num::ParseIntError;

use alloy_contract::Error as ContractError;
use alloy_json_rpc::RpcError;
use alloy_primitives::ruint::FromUintError;
use alloy_transport::TransportErrorKind;
use block_building::http_client::HttpError;
use k256::ecdsa::Error as EcdsaError;
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

    #[error("{0}")]
    ParseInt(#[from] ParseIntError),

    #[error("{0}")]
    FromUInt64(#[from] FromUintError<u64>),

    #[error("{0}")]
    FromUInt128(#[from] FromUintError<u128>),
}

pub type PreconferResult<T> = Result<T, PreconferError>;
