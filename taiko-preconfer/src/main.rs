use std::str::FromStr;

use alloy_primitives::Address;
use alloy_rpc_types::BlockNumberOrTag;
use alloy_rpc_types_engine::JwtSecret;

mod error;
use crate::error::PreconferResult;

mod rpc;
use block_building::{
    rpc_client::{RpcClient, get_block},
    taiko::hekla::GAS_LIMIT,
};

use crate::rpc::{flatten_mempool_txs, get_auth_client, get_client, get_mempool_txs};

const HEKLA_URL: &str = "https://rpc.hekla.taiko.xyz";
const LOCAL_TAIKO_URL: &str = "http://37.27.222.77:28551";

#[tokio::main]
async fn main() -> PreconferResult<()> {
    let client = RpcClient::new(get_client(HEKLA_URL)?);
    let full_tx = true;
    let block = get_block(&client, BlockNumberOrTag::Latest, full_tx).await?;

    println!("Latest Block Header:");
    println!("Number: {:?}", block.header.number);
    println!("Hash: {:?}", block.header.hash);
    println!("Parent Hash: {:?}", block.header.parent_hash);
    println!("Timestamp: {:?}", block.header.timestamp);
    let jwt_secret =
        JwtSecret::from_hex("654c8ed1da58823433eb6285234435ed52418fa9141548bca1403cc0ad519432")
            .unwrap();

    let auth_client = get_auth_client(LOCAL_TAIKO_URL, jwt_secret)?;
    let mempool_tx_lists = get_mempool_txs(
        &auth_client,
        Address::from_str("0xA6f54d514592187F0aE517867466bfd2CCfde4B0").unwrap(),
        10000,
        GAS_LIMIT,
        10000,
        vec![],
        10000,
    )
    .await
    .unwrap();
    let mempool_txs = flatten_mempool_txs(mempool_tx_lists);
    println!("#mempool tx lists {:?}", mempool_txs.len());
    Ok(())
}
