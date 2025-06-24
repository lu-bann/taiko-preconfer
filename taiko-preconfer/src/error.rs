use thiserror::Error;

#[derive(Error, Debug)]
pub enum ApplicationError {
    #[error("{0}")]
    Config(#[from] preconfirmation::preconf::config::ConfigError),

    #[error("{0}")]
    TaikoL1Client(#[from] preconfirmation::taiko::taiko_l1_client::TaikoL1ClientError),

    #[error("{0}")]
    TaikoL2Client(#[from] preconfirmation::taiko::taiko_l2_client::TaikoL2ClientError),

    #[error("{0}")]
    Confirmation(#[from] preconfirmation::preconf::confirmation_strategy::ConfirmationError),

    #[error("{0}")]
    DotEnv(#[from] dotenv::Error),

    #[error("{0}")]
    Rpc(#[from] alloy_json_rpc::RpcError<alloy_transport::TransportErrorKind>),

    #[error("{0}")]
    UrlParse(#[from] url::ParseError),

    #[error("{0}")]
    TryFromInt(#[from] std::num::TryFromIntError),

    #[error("{0}")]
    BlockBuilder(#[from] preconfirmation::preconf::BlockBuilderError),

    #[error("{0}")]
    SystemTime(#[from] std::time::SystemTimeError),

    #[error("{0}")]
    FromHex(#[from] alloy_primitives::hex::FromHexError),

    #[error("{0}")]
    Signer(#[from] alloy_signer_local::LocalSignerError),
}

pub type ApplicationResult<T> = Result<T, ApplicationError>;
