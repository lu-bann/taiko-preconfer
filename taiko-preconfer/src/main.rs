use alloy_primitives::Address;
use alloy_provider::{Provider, ProviderBuilder, WsConnect};
use alloy_rpc_types::{Block, Header, Transaction};
use alloy_rpc_types_engine::JwtSecret;
use futures::{
    future::BoxFuture,
    {FutureExt, StreamExt},
};
use std::sync::Arc;
use std::time::Duration;
use tokio::{join, sync::Mutex, time::sleep};
use tracing::info;

use block_building::{
    http_client::HttpClient,
    preconf::{Config, Preconfer},
    rpc_client::RpcClient,
    taiko::{
        contracts::TaikoAnchorInstance,
        hekla::{CHAIN_ID, addresses::get_taiko_anchor_address, get_basefee_config_v2},
        taiko_client::{ITaikoClient, TaikoClient},
    },
};

mod rpc;
use crate::rpc::{get_auth_client, get_client};

mod error;
use crate::error::{ApplicationError, ApplicationResult};

const HEKLA_URL: &str = "https://rpc.hekla.taiko.xyz";
const LOCAL_TAIKO_URL: &str = "http://37.27.222.77:28551";
const L1_URL: &str = "https://rpc.holesky.luban.wtf";
const WS_HEKLA_URL: &str = "ws://37.27.222.77:28546";
const WS_L1_URL: &str = "wss://rpc.holesky.luban.wtf/ws";

async fn stream_block_headers<'a, T: Fn(Header) -> BoxFuture<'a, ApplicationResult<()>>>(
    url: &str,
    f: T,
) -> ApplicationResult<()> {
    info!("Subscribe to headers at {url}");
    let ws = WsConnect::new(url);
    let provider = ProviderBuilder::new().connect_ws(ws).await?;
    let mut stream = provider.subscribe_blocks().await?;

    while let Ok(header) = stream.recv().await {
        f(header).await?;
    }
    Err(ApplicationError::WsConnectionLost {
        url: url.to_string(),
    })
}

async fn stream_block_headers_with_builder<
    'a,
    L1Client: HttpClient,
    Taiko: ITaikoClient,
    T: Fn(Header, Arc<Mutex<Preconfer<L1Client, Taiko>>>) -> BoxFuture<'a, ApplicationResult<()>>,
>(
    url: &str,
    f: T,
    block_builder: Arc<Mutex<Preconfer<L1Client, Taiko>>>,
) -> ApplicationResult<()> {
    info!("Subscribe to headers at {url}");
    let ws = WsConnect::new(url);
    let provider = ProviderBuilder::new().connect_ws(ws).await?;
    let mut stream = provider.subscribe_blocks().await?;

    while let Ok(header) = stream.recv().await {
        f(header, block_builder.clone()).await?;
    }
    Err(ApplicationError::WsConnectionLost {
        url: url.to_string(),
    })
}

async fn stream_block_headers_into<
    'a,
    T: Fn(Header, Arc<Mutex<u64>>) -> BoxFuture<'a, ApplicationResult<()>>,
>(
    url: &str,
    f: T,
    current: Arc<Mutex<u64>>,
) -> ApplicationResult<()> {
    info!("Subscribe to headers at {url}");
    let ws = WsConnect::new(url);
    let provider = ProviderBuilder::new().connect_ws(ws).await?;
    let mut stream = provider.subscribe_blocks().await?;

    while let Ok(header) = stream.recv().await {
        f(header, current.clone()).await?;
    }
    Err(ApplicationError::WsConnectionLost {
        url: url.to_string(),
    })
}

async fn stream_blocks<'a, T: Fn(Block) -> BoxFuture<'a, ApplicationResult<()>>>(
    url: &str,
    f: T,
) -> ApplicationResult<()> {
    info!("Subscribe to full blocks at {url}");
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
    Err(ApplicationError::WsConnectionLost {
        url: url.to_string(),
    })
}

async fn stream_pending_transactions<
    'a,
    T: FnMut(Transaction, Arc<Mutex<Vec<Transaction>>>) -> BoxFuture<'a, ApplicationResult<()>>,
>(
    url: &str,
    f: T,
    mempool_txs: Arc<Mutex<Vec<Transaction>>>,
) -> ApplicationResult<()> {
    let mut f = f;
    info!("Subscribe to pending transactions at {url}");
    let ws = WsConnect::new(url);
    let provider = ProviderBuilder::new().connect_ws(ws).await?;
    let mut stream = provider.subscribe_full_pending_transactions().await?;

    while let Ok(tx) = stream.recv().await {
        f(tx, mempool_txs.clone()).await?;
    }
    Err(ApplicationError::WsConnectionLost {
        url: url.to_string(),
    })
}

async fn print_mempool_txs_len(mempool_transactions: Arc<Mutex<Vec<Transaction>>>) {
    loop {
        sleep(Duration::from_secs(5)).await;
        let n = mempool_transactions.lock().await;
        info!("=== #TX: {} === ", n.len());
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
                mempool_txs.lock().await.push(tx);
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

async fn run_preconfer() -> ApplicationResult<()> {
    tracing_subscriber::fmt().with_target(false).init();

    let l2_client = RpcClient::new(get_client(HEKLA_URL)?);
    let jwt_secret =
        JwtSecret::from_hex("654c8ed1da58823433eb6285234435ed52418fa9141548bca1403cc0ad519432")
            .unwrap();
    let auth_client = RpcClient::new(get_auth_client(LOCAL_TAIKO_URL, jwt_secret)?);
    let l1_client = RpcClient::new(get_client(L1_URL)?);
    let taiko_anchor_address = get_taiko_anchor_address();
    let provider = ProviderBuilder::new().connect(HEKLA_URL).await?;
    let taiko_anchor = TaikoAnchorInstance::new(taiko_anchor_address, provider.clone());

    let taiko = TaikoClient::new(
        l2_client,
        auth_client,
        taiko_anchor,
        provider,
        get_basefee_config_v2(),
        CHAIN_ID,
    );
    let block_builder = Arc::new(Mutex::new(Preconfer::new(
        Config::default(),
        l1_client,
        taiko,
        Address::random(),
    )));
    let shared_last_l1_block_number = block_builder.lock().await.shared_last_l1_block_number();

    let process_l1_header = {
        |header: Header, current: Arc<Mutex<u64>>| {
            async move {
                let num = header.number;
                let hash = header.hash;

                info!("L1 🗣 #{:<10} {}", num, hash);
                *current.lock().await = header.number;
                Ok(())
            }
            .boxed()
        }
    };

    let process_l2_header = {
        |header: Header, block_builder: Arc<Mutex<Preconfer<RpcClient, TaikoClient>>>| {
            async move {
                let num = header.number;
                let hash = header.hash;

                info!("L2 🗣 #{:<10} {}", num, hash);

                block_builder.lock().await.build_block(header).await?;
                Ok(())
            }
            .boxed()
        }
    };

    let _ = join!(
        stream_block_headers_into(
            WS_L1_URL,
            process_l1_header,
            shared_last_l1_block_number.clone()
        ),
        stream_block_headers_with_builder(WS_HEKLA_URL, process_l2_header, block_builder),
    );

    Ok(())
}

#[tokio::main]
async fn main() -> ApplicationResult<()> {
    run_preconfer().await
}
