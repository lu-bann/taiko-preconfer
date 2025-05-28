#![allow(dead_code)]

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

use crate::error::PreconferResult;

pub fn get_client(url: &str) -> PreconferResult<AlloyClient> {
    let transport = Http::new(Url::parse(url)?);
    Ok(AlloyClient::new(transport, false))
}

pub fn get_auth_client(url: &str, jwt_secret: JwtSecret) -> PreconferResult<AlloyClient> {
    let hyper_client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();
    let auth_layer = AuthLayer::new(jwt_secret);
    let service = ServiceBuilder::new()
        .layer(auth_layer)
        .service(hyper_client);

    let layer_transport = HyperClient::with_service(service);
    let http_hyper = Http::with_client(layer_transport, Url::parse(url)?);

    Ok(AlloyClient::new(http_hyper, true))
}
