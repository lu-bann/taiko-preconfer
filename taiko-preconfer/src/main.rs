use alloy_rpc_client::RpcClient;
use alloy_rpc_types::{Block, BlockNumberOrTag};
use alloy_transport_http::Http;
use serde_json::json;
use url::Url;

mod error;
use crate::error::{PreconferError, PreconferResult};

const HEKLA_URL: &str = "https://rpc.hekla.taiko.xyz";

const GET_LATEST_BLOCK: &str = "eth_getBlockByNumber";

fn get_client(url: &str) -> PreconferResult<RpcClient> {
    let transport = Http::new(Url::parse(url)?);
    Ok(RpcClient::new(transport, false))
}

async fn get_latest_block(client: RpcClient) -> PreconferResult<Block> {
    let request_full_tx_objects = false;
    let params = json!([BlockNumberOrTag::Latest, request_full_tx_objects]);

    let rpc_call = client.request(GET_LATEST_BLOCK, params.clone());
    let method = rpc_call.method().to_string();
    let params = rpc_call.request().params.clone();
    let block: Option<Block> = rpc_call.await?;
    block.ok_or(PreconferError::FailedRPCRequest { method, params })
}

#[tokio::main]
async fn main() -> PreconferResult<()> {
    let client = get_client(HEKLA_URL)?;
    let block = get_latest_block(client).await?;

    println!("Latest Block Header:");
    println!("Number: {:?}", block.header.number);
    println!("Hash: {:?}", block.header.hash);
    println!("Parent Hash: {:?}", block.header.parent_hash);
    println!("Timestamp: {:?}", block.header.timestamp);

    Ok(())
}
