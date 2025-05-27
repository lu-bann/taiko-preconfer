use alloy_consensus::Header;
use alloy_json_rpc::RpcRecv;
use alloy_rpc_types::Header as RpcHeader;
use serde_json::{Value as JsonValue, from_value, to_value};

use crate::{
    http_client::{HttpClient, HttpError},
    taiko::preconf_blocks::{BuildPreconfBlockResponse, PRECONF_BLOCKS},
};

pub struct DummyClient;

impl HttpClient for DummyClient {
    async fn request<Resp: RpcRecv>(
        &self,
        method: String,
        params: JsonValue,
    ) -> Result<Resp, HttpError> {
        if method == PRECONF_BLOCKS {
            return Ok(from_value(
                to_value(BuildPreconfBlockResponse {
                    block_header: RpcHeader::new(Header::default()),
                })
                .unwrap(),
            )
            .unwrap());
        }
        Err(HttpError::FailedRPCRequest { method, params })
    }
}
