use thiserror::Error;

#[derive(Error, Debug)]
pub enum ApplicationError {
    #[error("{0}")]
    Config(#[from] block_building::preconf::config::ConfigError),

    #[error("{0}")]
    DotEnv(#[from] dotenv::Error),

    #[error("{0}")]
    Rpc(#[from] alloy_json_rpc::RpcError<alloy_transport::TransportErrorKind>),

    #[error("{0}")]
    UrlParse(#[from] url::ParseError),

    #[error("{0}")]
    TryFromInt(#[from] std::num::TryFromIntError),

    #[error("{0}")]
    Preconfer(#[from] block_building::preconf::PreconferError),

    #[error("{0}")]
    SystemTime(#[from] std::time::SystemTimeError),

    #[error("Web socket connection lost at {url}.")]
    WsConnectionLost { url: String },
}

pub type ApplicationResult<T> = Result<T, ApplicationError>;
