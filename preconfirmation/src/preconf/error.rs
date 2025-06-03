use thiserror::Error;

#[derive(Debug, Error)]
pub enum PreconferError {
    #[error("{0}")]
    Compression(#[from] libdeflater::CompressionError),

    #[error("{0}")]
    Http(#[from] crate::client::HttpError),

    #[error("{0}")]
    Sign(#[from] alloy_signer::Error),

    #[error("{0}")]
    SystemTime(#[from] std::time::SystemTimeError),

    #[error("{0}")]
    TaikoClient(#[from] crate::taiko::taiko_client::TaikoClientError),

    #[error("{0}")]
    TaikoL1Client(#[from] crate::taiko::taiko_l1_client::TaikoL1ClientError),

    #[error("{0}")]
    TryLock(#[from] tokio::sync::TryLockError),

    #[error("Missing parent header")]
    MissingParentHeader,
}

pub type PreconferResult<T> = Result<T, PreconferError>;
