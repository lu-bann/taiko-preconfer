use std::{sync::Arc, time::Duration};

use alloy_consensus::Header;
use alloy_primitives::Address;
use alloy_provider::{Provider, ProviderBuilder, WsConnect};
use alloy_rpc_types_eth::Block;
use futures::{Stream, StreamExt, future::BoxFuture, pin_mut};
use preconfirmation::{
    client::reqwest::get_block_by_id,
    preconf::config::Config,
    stream::{
        get_block_polling_stream, get_block_stream, get_confirmed_id_polling_stream,
        get_header_polling_stream, get_header_stream, get_id_stream, get_l2_head_stream,
        stream_l2_headers, to_boxed,
    },
    taiko::{anchor::ValidAnchor, contracts::TaikoInboxInstance, taiko_l1_client::TaikoL1Client},
    util::{log_error, now_as_secs},
    verification::{LastBatchVerifier, TaikoInboxError},
};
use tokio::{join, sync::RwLock};
use tracing::{info, instrument};

use crate::error::ApplicationResult;

async fn stream_l1_headers<
    'a,
    E,
    T: Fn(Header, Arc<RwLock<ValidAnchor<TaikoL1Client>>>) -> BoxFuture<'a, Result<(), E>>,
>(
    stream: impl Stream<Item = Header>,
    f: T,
    valid_anchor: Arc<RwLock<ValidAnchor<TaikoL1Client>>>,
) -> Result<(), E> {
    pin_mut!(stream);
    while let Some(header) = stream.next().await {
        f(header, valid_anchor.clone()).await?;
    }
    Ok(())
}

async fn store_header(
    header: Header,
    current: Arc<RwLock<Header>>,
    msg: String,
) -> ApplicationResult<()> {
    info!(
        "{msg} 🗣 #{:<10} timestamp={} now={} state_root={:?} gas_used={}",
        header.number,
        header.timestamp,
        now_as_secs(),
        header.state_root,
        header.gas_used
    );
    *current.write().await = header;
    Ok(())
}

async fn store_valid_anchor(
    header: Header,
    valid_anchor: Arc<RwLock<ValidAnchor<TaikoL1Client>>>,
) -> ApplicationResult<()> {
    info!(
        "L1 🗣 #{:<10} timestamp={} now={} state_root={:?} gas_used={}",
        header.number,
        header.timestamp,
        now_as_secs(),
        header.state_root,
        header.gas_used
    );
    valid_anchor
        .write()
        .await
        .update_block_number(header.number)
        .await?;
    Ok(())
}

async fn store_header_l2(header: Header, current: Arc<RwLock<Header>>) -> ApplicationResult<()> {
    store_header(header, current, "L2".to_string()).await
}

fn store_valid_anchor_boxed<'a>(
    header: Header,
    valid_anchor_id: Arc<RwLock<ValidAnchor<TaikoL1Client>>>,
) -> BoxFuture<'a, ApplicationResult<()>> {
    Box::pin(store_valid_anchor(header, valid_anchor_id))
}

fn store_header_boxed_l2<'a>(
    header: Header,
    current: Arc<RwLock<Header>>,
) -> BoxFuture<'a, ApplicationResult<()>> {
    to_boxed(header, current, store_header_l2)
}

pub async fn create_header_stream(
    client_url: &str,
    ws_url: &str,
    poll_period: Duration,
) -> ApplicationResult<impl Stream<Item = Header>> {
    let provider = ProviderBuilder::new().connect(client_url).await?;
    let polling_stream = get_header_polling_stream(provider, poll_period);

    let ws = WsConnect::new(ws_url);
    let provider = ProviderBuilder::new().connect_ws(ws).await?;
    let subscription = provider.subscribe_blocks().await?;
    let ws_stream = subscription.into_stream().map(|header| header.inner);
    Ok(get_header_stream(polling_stream, ws_stream))
}

pub async fn create_l2_head_stream(
    config: &Config,
    preconfer_address: Address,
    latest_confirmed_block_id: u64,
    unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>>,
    valid_anchor: Arc<RwLock<ValidAnchor<TaikoL1Client>>>,
    taiko_inbox: TaikoInboxInstance,
) -> ApplicationResult<impl Stream<Item = Result<Header, TaikoInboxError>>> {
    let l2_block_stream =
        create_block_stream(&config.l2_client_url, &config.l2_ws_url, config.poll_period).await?;
    let valid_anchor = valid_anchor.clone();
    let batch_proposed_stream = taiko_inbox
        .BatchProposed_filter()
        .subscribe()
        .await?
        .into_stream()
        .filter_map(move |result| {
            let local_valid_anchor = valid_anchor.clone();
            async move {
                match result {
                    Ok((batch_proposed, _log)) => {
                        log_error(
                            local_valid_anchor
                                .write()
                                .await
                                .update_last_anchor_id(batch_proposed.info.anchorBlockId)
                                .await,
                            "Failed to update anchor from BatchProposed",
                        );
                        Some(batch_proposed.info.lastBlockId)
                    }
                    Err(_) => None,
                }
            }
        });
    let confirmed_id_polling_stream =
        get_confirmed_id_polling_stream(taiko_inbox.clone(), config.poll_period)
            .filter_map(|id| async { id.ok() });
    let id_stream = get_id_stream(confirmed_id_polling_stream, batch_proposed_stream);

    let get_header_call = |id: Option<u64>| {
        let url = config.l2_client_url.clone();
        async move { get_block_by_id(url.clone(), id).await }
    };
    let last_batch_verifier =
        LastBatchVerifier::new(taiko_inbox, preconfer_address, config.use_blobs);
    Ok(get_l2_head_stream(
        l2_block_stream,
        id_stream,
        unconfirmed_l2_blocks,
        get_header_call,
        Some(latest_confirmed_block_id),
        last_batch_verifier,
    )
    .await)
}

pub async fn create_block_stream(
    client_url: &str,
    ws_url: &str,
    poll_period: Duration,
) -> ApplicationResult<impl Stream<Item = Block>> {
    let provider = ProviderBuilder::new().connect(client_url).await?;
    let polling_stream = get_block_polling_stream(provider, poll_period);

    let ws = WsConnect::new(ws_url);
    let provider = ProviderBuilder::new().connect_ws(ws).await?;
    let ws_stream = provider
        .subscribe_full_blocks()
        .into_stream()
        .await?
        .filter_map(|res| async move { res.ok() });
    Ok(get_block_stream(polling_stream, ws_stream))
}

#[instrument(name = "🔄", skip_all)]
pub async fn run<
    L1Stream: Stream<Item = Header>,
    L2Stream: Stream<Item = Result<Header, TaikoInboxError>>,
>(
    l1_header_stream: L1Stream,
    l2_header_stream: L2Stream,
    shared_last_l2_header: Arc<RwLock<Header>>,
    valid_anchor_id: Arc<RwLock<ValidAnchor<TaikoL1Client>>>,
) -> ApplicationResult<()> {
    let (l1_result, l2_result) = join!(
        stream_l1_headers(l1_header_stream, store_valid_anchor_boxed, valid_anchor_id,),
        stream_l2_headers(
            l2_header_stream,
            store_header_boxed_l2,
            shared_last_l2_header
        ),
    );
    l1_result?;
    l2_result
}
