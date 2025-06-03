use alloy_json_rpc::RpcRecv;
use alloy_primitives::Bytes;
use alloy_rpc_client::RpcClient as AlloyClient;
use alloy_rpc_types_engine::JwtSecret;
use alloy_transport_http::{
    AuthLayer, Http, HyperClient,
    hyper_util::{client::legacy::Client, rt::TokioExecutor},
};
use http_body_util::Full;
use tower::ServiceBuilder;
use url::Url;

use crate::client::{HttpClient, HttpError};

#[derive(Debug)]
pub struct RpcClient {
    rpc_client: AlloyClient,
}

impl RpcClient {
    pub const fn new(client: AlloyClient) -> Self {
        Self { rpc_client: client }
    }
}

impl HttpClient for RpcClient {
    async fn request<Resp: RpcRecv>(
        &self,
        method: String,
        params: serde_json::Value,
    ) -> Result<Resp, HttpError> {
        Ok(self.rpc_client.request(method, params).await?)
    }
}

pub fn get_alloy_client(url: &str, is_local: bool) -> Result<AlloyClient, url::ParseError> {
    let transport = Http::new(Url::parse(url)?);
    Ok(AlloyClient::new(transport, is_local))
}

pub fn get_alloy_auth_client(
    url: &str,
    jwt_secret: JwtSecret,
    is_local: bool,
) -> Result<AlloyClient, url::ParseError> {
    let hyper_client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();
    let auth_layer = AuthLayer::new(jwt_secret);
    let service = ServiceBuilder::new()
        .layer(auth_layer)
        .service(hyper_client);

    let layer_transport = HyperClient::with_service(service);
    let transport = Http::with_client(layer_transport, Url::parse(url)?);
    Ok(AlloyClient::new(transport, is_local))
}
