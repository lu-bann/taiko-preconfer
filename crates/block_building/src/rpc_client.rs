use alloy_json_rpc::RpcRecv;
use alloy_rpc_client::RpcClient as AlloyClient;

use crate::http_client::{HttpClient, HttpError};

pub struct RpcClient {
    rpc_client: AlloyClient,
}

impl RpcClient {
    pub fn new(client: AlloyClient) -> Self {
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
