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
    BlockBuilder(#[from] preconfirmation::preconf::BlockBuilderError),

    #[error("{0}")]
    InboxError(#[from] preconfirmation::verification::TaikoInboxError),

    #[error("{0}")]
    DotEnv(#[from] dotenv::Error),

    #[error("{0}")]
    Rpc(#[from] alloy::rpc::json_rpc::RpcError<alloy::transports::TransportErrorKind>),

    #[error("{0}")]
    UrlParse(#[from] url::ParseError),

    #[error("{0}")]
    TryFromInt(#[from] std::num::TryFromIntError),

    #[error("{0}")]
    Signer(#[from] alloy::signers::local::LocalSignerError),

    #[error("{0}")]
    SolType(#[from] alloy::sol_types::Error),

    #[error("{0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("{0}")]
    Contract(#[from] alloy::contract::Error),
}

pub type ApplicationResult<T> = Result<T, ApplicationError>;
