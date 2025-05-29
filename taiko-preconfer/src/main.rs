use alloy_primitives::{Address, B256, Bytes};
use alloy_provider::{Provider, ProviderBuilder, WsConnect, network::TransactionBuilder};
use alloy_rpc_types::{Block, Header, Transaction, TransactionRequest};
use alloy_rpc_types_engine::JwtSecret;
use alloy_signer_local::PrivateKeySigner;
use futures::{
    future::BoxFuture,
    {FutureExt, StreamExt},
};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::{join, sync::Mutex, time::sleep};
use tracing::info;

use block_building::{
    compression::compress,
    http_client::{HttpClient, get_header_by_id, get_nonce},
    rpc_client::RpcClient,
    taiko::{
        contracts::{TaikoAnchorInstance, taiko_wrapper::BlockParams},
        hekla::{
            CHAIN_ID,
            addresses::{GOLDEN_TOUCH_ADDRESS, get_taiko_anchor_address, get_taiko_inbox_address},
            get_basefee_config_v2,
        },
        propose_batch::create_propose_batch_params,
        taiko_client::{ITaikoClient, TaikoClient},
    },
};

mod error;
use crate::error::PreconferResult;

mod rpc;
use crate::rpc::{get_auth_client, get_client};

mod add_anchor_transaction;
use crate::add_anchor_transaction::{Config, get_anchor_id, get_signed_eip1559_tx, get_timestamp};

const HEKLA_URL: &str = "https://rpc.hekla.taiko.xyz";
const LOCAL_TAIKO_URL: &str = "http://37.27.222.77:28551";
const L1_URL: &str = "https://rpc.holesky.luban.wtf";
const WS_HEKLA_URL: &str = "ws://37.27.222.77:28546";
const WS_L1_URL: &str = "wss://rpc.holesky.luban.wtf/ws";

async fn stream_block_headers<'a, T: Fn(Header) -> BoxFuture<'a, PreconferResult<()>>>(
    url: &str,
    f: T,
) -> PreconferResult<()> {
    info!("Subscribe to headers at {url}");
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

async fn stream_block_headers_with_builder<
    'a,
    L1Client: HttpClient,
    Taiko: ITaikoClient,
    T: Fn(Header, Arc<Mutex<BlockBuilder<L1Client, Taiko>>>) -> BoxFuture<'a, PreconferResult<()>>,
>(
    url: &str,
    f: T,
    block_builder: Arc<Mutex<BlockBuilder<L1Client, Taiko>>>,
) -> PreconferResult<()> {
    info!("Subscribe to headers at {url}");
    let ws = WsConnect::new(url);
    let provider = ProviderBuilder::new().connect_ws(ws).await?;
    let mut stream = provider.subscribe_blocks().await?;

    while let Ok(header) = stream.recv().await {
        f(header, block_builder.clone()).await?;
    }
    Err(error::PreconferError::WsConnectionLost {
        url: url.to_string(),
    })
}

async fn stream_block_headers_into<
    'a,
    T: Fn(Header, Arc<Mutex<u64>>) -> BoxFuture<'a, PreconferResult<()>>,
>(
    url: &str,
    f: T,
    current: Arc<Mutex<u64>>,
) -> PreconferResult<()> {
    info!("Subscribe to headers at {url}");
    let ws = WsConnect::new(url);
    let provider = ProviderBuilder::new().connect_ws(ws).await?;
    let mut stream = provider.subscribe_blocks().await?;

    while let Ok(header) = stream.recv().await {
        f(header, current.clone()).await?;
    }
    Err(error::PreconferError::WsConnectionLost {
        url: url.to_string(),
    })
}

async fn stream_blocks<'a, T: Fn(Block) -> BoxFuture<'a, PreconferResult<()>>>(
    url: &str,
    f: T,
) -> PreconferResult<()> {
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
    Err(error::PreconferError::WsConnectionLost {
        url: url.to_string(),
    })
}

async fn stream_pending_transactions<
    'a,
    T: FnMut(Transaction, Arc<Mutex<Vec<Transaction>>>) -> BoxFuture<'a, PreconferResult<()>>,
>(
    url: &str,
    f: T,
    mempool_txs: Arc<Mutex<Vec<Transaction>>>,
) -> PreconferResult<()> {
    let mut f = f;
    info!("Subscribe to pending transactions at {url}");
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

#[derive(Debug)]
struct BlockBuilder<L1Client: HttpClient, Taiko: ITaikoClient> {
    last_l1_block_number: Arc<Mutex<u64>>,
    config: Config,
    l1_client: L1Client,
    taiko: Taiko,
    address: Address,
}

impl<L1Client: HttpClient, Taiko: ITaikoClient> BlockBuilder<L1Client, Taiko> {
    pub fn new(config: Config, l1_client: L1Client, taiko: Taiko, address: Address) -> Self {
        Self {
            last_l1_block_number: Arc::new(Mutex::new(0u64)),
            config,
            l1_client,
            taiko,
            address,
        }
    }

    pub fn shared_last_l1_block_number(&self) -> Arc<Mutex<u64>> {
        self.last_l1_block_number.clone()
    }

    pub fn last_l1_block_number(&self) -> PreconferResult<u64> {
        Ok(self.last_l1_block_number.try_lock().map(|guard| *guard)?)
    }

    async fn wait_until_next_block(&self, current_block_timestamp: u64) -> PreconferResult<()> {
        let now = SystemTime::now();
        let desired_next_block_time = UNIX_EPOCH
            .checked_add(Duration::from_secs(current_block_timestamp))
            .map(|x| {
                x.checked_add(self.config.l2_block_time)
                    .unwrap_or(UNIX_EPOCH)
            })
            .unwrap_or(UNIX_EPOCH);
        let remaining = desired_next_block_time.duration_since(now)?;
        sleep(remaining).await;
        Ok(())
    }

    pub async fn build_block(&self, parent_header: Header) -> PreconferResult<()> {
        self.wait_until_next_block(parent_header.timestamp).await?;

        let start = SystemTime::now();

        let last_l1_block_number = self.last_l1_block_number()?;
        info!(
            "build block {}, l1 {}",
            parent_header.number + 1,
            last_l1_block_number
        );
        let now = get_timestamp();
        info!("t: now={} parent={}", now, parent_header.timestamp);

        let anchor_block_id = get_anchor_id(last_l1_block_number, self.config.anchor_id_lag);
        let (anchor_header, golden_touch_nonce, base_fee) = join!(
            get_header_by_id(&self.l1_client, anchor_block_id),
            self.taiko.get_nonce(GOLDEN_TOUCH_ADDRESS),
            self.taiko.get_base_fee(parent_header.gas_used, now),
        );

        let base_fee: u128 = base_fee?;
        let mut txs = self
            .taiko
            .get_mempool_txs(self.address, base_fee as u64)
            .await?;
        info!("#txs in mempool: {}", txs.len());

        let anchor_tx = self.taiko.get_signed_anchor_tx(
            anchor_block_id,
            anchor_header?.state_root,
            parent_header.gas_used as u32,
            golden_touch_nonce?,
            base_fee,
        )?;
        txs.insert(0, anchor_tx);

        let _header = self
            .taiko
            .publish_preconfirmed_transactions(
                self.address,
                base_fee as u64,
                now, // after await, recompute?
                &parent_header,
                txs.clone(),
            )
            .await?;

        let number_of_blobs = 0u8;
        let parent_meta_hash = B256::ZERO;
        let coinbase = parent_header.beneficiary;
        let tx_bytes = Bytes::from(compress(txs.clone())?);
        let block_params = vec![BlockParams {
            numTransactions: txs.len() as u16,
            timeShift: 0,
            signalSlots: vec![],
        }];
        let propose_batch_params = create_propose_batch_params(
            self.address,
            tx_bytes,
            block_params,
            parent_meta_hash,
            anchor_block_id,
            parent_header.timestamp,
            coinbase,
            number_of_blobs,
        );

        let signer = PrivateKeySigner::random();
        let taiko_inbox_address = get_taiko_inbox_address();
        let tx = TransactionRequest::default()
            .with_to(taiko_inbox_address)
            .with_input(propose_batch_params.clone())
            .with_from(signer.address());
        let signer_str = signer.address().to_string();
        let (nonce, gas_limit, fee_estimate) = join!(
            get_nonce(&self.l1_client, &signer_str),
            self.taiko.estimate_gas(tx),
            self.taiko.estimate_eip1559_fees(),
        );
        let fee_estimate = fee_estimate?;

        let signed_tx = get_signed_eip1559_tx(
            taiko_inbox_address,
            propose_batch_params,
            nonce?,
            gas_limit?,
            fee_estimate.max_fee_per_gas,
            fee_estimate.max_priority_fee_per_gas,
            &signer,
        )?;

        info!("signed propose batch tx {signed_tx:?}");
        let end = SystemTime::now();
        info!(
            "elapsed: {} ms",
            end.duration_since(start).unwrap().as_millis()
        );
        Ok(())
    }
}

async fn run_preconfer() -> PreconferResult<()> {
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
    let block_builder = Arc::new(Mutex::new(BlockBuilder::new(
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
        |header: Header, block_builder: Arc<Mutex<BlockBuilder<RpcClient, TaikoClient>>>| {
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
async fn main() -> PreconferResult<()> {
    run_preconfer().await
}
