use crate::http_client::HttpError;

//#[cfg_attr(test, mockall::automock)]
pub trait EvmClient {
    fn get_nonce(&self, address: &str) -> impl Future<Output = Result<u64, HttpError>>;
}
