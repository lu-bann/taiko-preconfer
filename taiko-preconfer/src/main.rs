use std::str::FromStr;

use add_anchor_transaction::{BlockBuilder, Config};
use alloy_primitives::Address;
use alloy_provider::ProviderBuilder;
use alloy_rpc_types::BlockNumberOrTag;
use alloy_rpc_types_engine::JwtSecret;
use alloy_sol_types::SolCall;
use rpc::{get_header, get_header_by_id, get_nonce};

mod error;
use crate::error::PreconferResult;

mod rpc;
use block_building::{
    rpc_client::{RpcClient, get_block},
    taiko::{
        contracts::TaikoAnchor::{TaikoAnchorInstance, anchorV3Call},
        hekla::{
            GAS_LIMIT,
            addresses::{GOLDEN_TOUCH_ADDRESS, get_taiko_anchor_address},
        },
    },
};
mod add_anchor_transaction;
use crate::rpc::{flatten_mempool_txs, get_auth_client, get_client, get_mempool_txs};

const HEKLA_URL: &str = "https://rpc.hekla.taiko.xyz";
const LOCAL_TAIKO_URL: &str = "http://37.27.222.77:28551";
const L1_URL: &str = "https://rpc.holesky.luban.wtf";

#[tokio::main]
async fn main() -> PreconferResult<()> {
    let client = RpcClient::new(get_client(HEKLA_URL)?);
    let l2_client = RpcClient::new(get_client(HEKLA_URL)?);
    let l1_client = RpcClient::new(get_client(L1_URL)?);
    let full_tx = true;
    let block = get_block(&client, BlockNumberOrTag::Latest, full_tx).await?;

    let config = Config::default();
    let taiko_anchor_address = get_taiko_anchor_address();
    let provider = ProviderBuilder::new().connect(HEKLA_URL).await?;
    let taiko_anchor = TaikoAnchorInstance::new(taiko_anchor_address, provider);
    let current_l1_header = get_header(&l1_client, BlockNumberOrTag::Latest).await?;
    let block_builder = BlockBuilder::new(l2_client, l1_client, config.anchor_id_lag, taiko_anchor);

    let parent_header = get_header_by_id(&client, block.header.number - 1).await?;
    let my_txs = block_builder.build_block(vec![], parent_header, current_l1_header).await?;

    let gt_nonce = get_nonce(&client, GOLDEN_TOUCH_ADDRESS).await?;
    println!("nonce {gt_nonce}");

    println!("Latest Block Header:");
    println!("Number: {:?}", block.header.number);
    println!("Hash: {:?}", block.header.hash);
    println!("Parent Hash: {:?}", block.header.parent_hash);
    println!("Timestamp: {:?}", block.header.timestamp);
    let jwt_secret =
        JwtSecret::from_hex("654c8ed1da58823433eb6285234435ed52418fa9141548bca1403cc0ad519432")
            .unwrap();

    let txs = block.transactions.into_transactions_vec();
    let inner = txs[0].inner.inner().clone().into_typed_transaction();
    let inner_call: anchorV3Call =
        <anchorV3Call as SolCall>::abi_decode(&inner.eip1559().unwrap().input).unwrap();
    let my_inner_call: anchorV3Call = <anchorV3Call as SolCall>::abi_decode(
        &my_txs[0].clone().into_typed_transaction().eip1559().unwrap().input,
    )
    .unwrap();
    println!("anchor ref: {:?}", txs[0].inner.inner());
    println!("anchor call: {inner_call:?}");
    println!("mine: {:?}", my_txs[0]);
    println!("mine call: {my_inner_call:?}");
    let auth_client = RpcClient::new(get_auth_client(LOCAL_TAIKO_URL, jwt_secret)?);
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
