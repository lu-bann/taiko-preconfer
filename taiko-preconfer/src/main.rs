use alloy_primitives::{Address, B256, Bytes};
use alloy_provider::{Provider, ProviderBuilder, WsConnect, network::TransactionBuilder};
use alloy_rpc_types::{Block, BlockNumberOrTag, TransactionRequest};
use alloy_rpc_types::{Header, Transaction};
use alloy_rpc_types_engine::JwtSecret;
use alloy_signer_local::PrivateKeySigner;
use alloy_sol_types::SolCall;
use futures::future::BoxFuture;
use futures::{FutureExt, StreamExt};
use tokio::time::sleep;

use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::join;
use tracing::{error, info};

mod error;
use crate::error::PreconferResult;

mod rpc;
use block_building::{
    compression::compress,
    dummy_client::DummyClient,
    http_client::{
        flatten_mempool_txs, get_block, get_header, get_header_by_id, get_mempool_txs, get_nonce,
    },
    rpc_client::RpcClient,
    taiko::{
        contracts::{
            TaikoAnchor::{TaikoAnchorInstance, anchorV3Call},
            taiko_wrapper::BlockParams,
        },
        hekla::{
            GAS_LIMIT,
            addresses::{GOLDEN_TOUCH_ADDRESS, get_taiko_anchor_address, get_taiko_inbox_address},
            get_basefee_config_v2,
        },
        preconf_blocks::{create_executable_data, publish_preconfirmed_transactions},
        propose_batch::create_propose_batch_params,
    },
};
mod add_anchor_transaction;
use crate::rpc::{get_auth_client, get_client};
use add_anchor_transaction::{
    Config, get_anchor_id, get_signed_eip1559_tx, get_timestamp, insert_anchor_transaction,
};

const HEKLA_URL: &str = "https://rpc.hekla.taiko.xyz";
const LOCAL_TAIKO_URL: &str = "http://37.27.222.77:28551";
const L1_URL: &str = "https://rpc.holesky.luban.wtf";
const WS_HEKLA_URL: &str = "ws://37.27.222.77:28546";
const WS_L1_URL: &str = "wss://rpc.holesky.luban.wtf/ws";

async fn stream_block_headers<T: Fn(Header) -> BoxFuture<'static, PreconferResult<()>>>(
    url: &str,
    f: T,
) -> PreconferResult<()> {
    info!("connect to {url}");
    let ws = WsConnect::new(url);
    let provider = ProviderBuilder::new().connect_ws(ws).await?;
    let mut stream = provider.subscribe_blocks().await?;

    while let Ok(header) = stream.recv().await {
        f(header).await?;
    }
    Err(error::PreconferError::WsConnectionLost {
        url: url.to_string(),
    })
}

async fn stream_blocks<T: Fn(Block) -> BoxFuture<'static, PreconferResult<()>>>(
    url: &str,
    f: T,
) -> PreconferResult<()> {
    info!("connect to {url}");
    let ws = WsConnect::new(url);
    let provider = ProviderBuilder::new().connect_ws(ws).await?;
    let mut stream = provider
        .subscribe_full_blocks()
        .full()
        .into_stream()
        .await?;

    while let Some(Ok(block)) = stream.next().await {
        f(block).await?;
    }
    Err(error::PreconferError::WsConnectionLost {
        url: url.to_string(),
    })
}

async fn stream_pending_transactions<
    T: FnMut(Transaction, Arc<Mutex<Vec<Transaction>>>) -> BoxFuture<'static, PreconferResult<()>>,
>(
    url: &str,
    f: T,
    mempool_txs: Arc<Mutex<Vec<Transaction>>>,
) -> PreconferResult<()> {
    let mut f = f;
    info!("connect to {url}");
    let ws = WsConnect::new(url);
    let provider = ProviderBuilder::new().connect_ws(ws).await?;
    let mut stream = provider.subscribe_full_pending_transactions().await?;

    while let Ok(tx) = stream.recv().await {
        f(tx, mempool_txs.clone()).await?;
    }
    Err(error::PreconferError::WsConnectionLost {
        url: url.to_string(),
    })
}

async fn print_mempool_txs_len(mempool_transactions: Arc<Mutex<Vec<Transaction>>>) {
    loop {
        sleep(Duration::from_secs(5)).await;
        match mempool_transactions.lock() {
            Ok(guard) => info!("=== #TX: {} === ", guard.len()),
            Err(err) => error!("Locking failed: {:?}", err),
        }
    }
}

#[allow(dead_code)]
async fn listen_to_header_streams() {
    tracing_subscriber::fmt().init();

    let mempool_transactions: Arc<Mutex<Vec<Transaction>>> = Arc::new(Mutex::new(vec![]));

    let process_l2_block = {
        |block: Block| {
            async move {
                let num = block.header.number;
                let hash = block.header.hash;

                info!("L2 🔨  #{:<10} {} {}", num, hash, block.transactions.len());
                Ok(())
            }
            .boxed()
        }
    };

    let process_l1_header = {
        |header: Header| {
            async move {
                let num = header.number;
                let hash = header.hash;

                info!("L1 🗣 #{:<10} {}", num, hash);
                Ok(())
            }
            .boxed()
        }
    };

    let process_l2_tx = {
        |tx: Transaction, mempool_txs: Arc<Mutex<Vec<Transaction>>>| {
            async move {
                info!("L2 ✉   #{:?}", tx.inner.hash());
                match mempool_txs.lock() {
                    Ok(mut guard) => guard.push(tx),
                    Err(err) => error!("Locking failed: {:?}", err),
                }
                Ok(())
            }
            .boxed()
        }
    };

    let _ = join!(
        stream_blocks(WS_HEKLA_URL, process_l2_block),
        stream_block_headers(WS_L1_URL, process_l1_header),
        stream_pending_transactions(WS_HEKLA_URL, process_l2_tx, mempool_transactions.clone()),
        print_mempool_txs_len(mempool_transactions)
    );
}

#[tokio::main]
async fn main() -> PreconferResult<()> {
    listen_to_header_streams().await;
    Ok(())
}

#[allow(dead_code)]
async fn test() -> PreconferResult<()> {
    let preconfer_address = Address::random();
    let client = RpcClient::new(get_client(HEKLA_URL)?);
    let l2_client = RpcClient::new(get_client(HEKLA_URL)?);
    let l1_client = RpcClient::new(get_client(L1_URL)?);
    let full_tx = true;
    let block = get_block(&client, BlockNumberOrTag::Latest, full_tx).await?;

    let config = Config::default();
    let taiko_anchor_address = get_taiko_anchor_address();
    let provider = ProviderBuilder::new().connect(HEKLA_URL).await?;
    let taiko_anchor = TaikoAnchorInstance::new(taiko_anchor_address, provider.clone());
    let current_l1_header = get_header(&l1_client, BlockNumberOrTag::Latest).await?;
    let base_fee_config = get_basefee_config_v2();
    let anchor_block_id = get_anchor_id(current_l1_header.number, config.anchor_id_lag);
    let anchor_header = get_header_by_id(&l1_client, anchor_block_id).await?;
    let nonce = get_nonce(&l2_client, GOLDEN_TOUCH_ADDRESS).await?;

    let parent_header = get_header_by_id(&client, block.header.number - 1).await?;
    let parent_block = get_block(
        &client,
        BlockNumberOrTag::Number(block.header.number - 1),
        full_tx,
    )
    .await?;
    println!("{:?}", parent_block.header);
    println!("{:?}", parent_header.hash_slow());
    println!("{:?}", block.header);
    let timestamp = get_timestamp();
    let base_fee: u128 = taiko_anchor
        .getBasefeeV2(
            parent_header.gas_used as u32,
            timestamp,
            base_fee_config.clone(),
        )
        .call()
        .await?
        .basefee_
        .try_into()?;

    let my_txs = insert_anchor_transaction(
        vec![],
        parent_header.clone(),
        anchor_header,
        base_fee,
        base_fee_config.clone(),
        nonce,
    )?;
    let executable_data = create_executable_data(
        base_fee as u64,
        parent_header.number + 1,
        base_fee_config.sharingPctg,
        preconfer_address,
        GAS_LIMIT,
        parent_header.hash_slow(),
        timestamp,
        my_txs.clone(),
    )?;
    println!("executable data {executable_data:?}");
    let dummy_client = DummyClient {};
    let end_of_sequencing = false;
    let dummy_header =
        publish_preconfirmed_transactions(&dummy_client, executable_data, end_of_sequencing)
            .await?;
    println!("header {dummy_header:?}");

    let number_of_blobs = 0u8;
    let parent_meta_hash = B256::ZERO;
    let coinbase = parent_header.beneficiary;
    let tx_bytes = Bytes::from(compress(my_txs.clone())?);
    let block_params = vec![BlockParams {
        numTransactions: my_txs.len() as u16,
        timeShift: 0,
        signalSlots: vec![],
    }];
    let propose_batch_params = create_propose_batch_params(
        preconfer_address,
        tx_bytes,
        block_params,
        parent_meta_hash,
        anchor_block_id,
        parent_header.timestamp,
        coinbase,
        number_of_blobs,
    );

    let signer = PrivateKeySigner::random();
    let nonce = get_nonce(&client, &signer.address().to_string()).await?;
    let taiko_inbox_address = get_taiko_inbox_address();
    let tx = TransactionRequest::default()
        .with_to(taiko_inbox_address)
        .with_input(propose_batch_params.clone())
        .with_from(signer.address());
    let gas_limit = provider.estimate_gas(tx).await?;
    let fee_estimate = provider.estimate_eip1559_fees().await?;
    let signed_tx = get_signed_eip1559_tx(
        taiko_inbox_address,
        propose_batch_params,
        nonce,
        gas_limit,
        fee_estimate.max_fee_per_gas,
        fee_estimate.max_priority_fee_per_gas,
        &signer,
    )?;

    println!("signed propose batch tx {signed_tx:?}");

    println!("nonce {nonce}");

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
        &my_txs[0]
            .clone()
            .into_typed_transaction()
            .eip1559()
            .unwrap()
            .input,
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
