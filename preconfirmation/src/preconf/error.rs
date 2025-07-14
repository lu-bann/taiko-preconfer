use thiserror::Error;

#[derive(Debug, Error)]
pub enum BlockBuilderError {
    #[error("{0}")]
    Compression(#[from] libdeflater::CompressionError),

    #[error("{0}")]
    Sign(#[from] alloy::signers::Error),

    #[error("{0}")]
    SystemTime(#[from] std::time::SystemTimeError),

    #[error("{0}")]
    TaikoClient(#[from] crate::taiko::taiko_l2_client::TaikoL2ClientError),

    #[error("{0}")]
    TaikoL1Client(#[from] crate::taiko::taiko_l1_client::TaikoL1ClientError),

    #[error("{0}")]
    TryLock(#[from] tokio::sync::TryLockError),
}

pub type BlockBuilderResult<T> = Result<T, BlockBuilderError>;
