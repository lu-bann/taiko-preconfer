use alloy_consensus::Header;
use alloy_primitives::Address;
use alloy_provider::{Provider, ProviderBuilder, WsConnect};
use alloy_rpc_types_engine::JwtSecret;
use block_building::header_stream::{get_header_stream, get_polling_stream};
use block_building::preconf::handover_start_buffer::{
    DummySequencingMonitor, end_of_handover_start_buffer,
};
use block_building::slot::SubSlot;
use block_building::slot_model::{HOLESKY_GENESIS_TIMESTAMP, SlotModel};
use block_building::slot_stream::{get_slot_stream, get_subslot_stream};
use futures::{FutureExt, future::BoxFuture};
use futures::{Stream, StreamExt, pin_mut};
use std::time::Duration;
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::time::Instant;
use tokio::{join, sync::Mutex};
use tracing::{debug, error, info, trace};

use block_building::{
    active_operator_model::ActiveOperatorModel,
    preconf::{Preconfer, config::Config},
    rpc_client::RpcClient,
    slot_model::HOLESKY_SLOT_MODEL,
    taiko::{
        contracts::TaikoAnchorInstance,
        hekla::{CHAIN_ID, addresses::get_taiko_anchor_address, get_basefee_config_v2},
        taiko_client::{ITaikoClient, TaikoClient},
        taiko_l1_client::{ITaikoL1Client, TaikoL1Client},
    },
    time_provider::{ITimeProvider, SystemTimeProvider},
};

mod rpc;
use crate::rpc::{get_auth_client, get_client};

mod error;
use crate::error::ApplicationResult;

fn get_next_slot_start(slot_time: &Duration) -> ApplicationResult<Instant> {
    let duration_now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let in_current_slot_ms: Duration =
        Duration::from_millis((duration_now.as_millis() % slot_time.as_millis()).try_into()?);
    let remaining = *slot_time - in_current_slot_ms;
    Ok(Instant::now() + remaining)
}

async fn stream_block_headers_into<
    'a,
    Value,
    T: Fn(Header, Arc<Mutex<Value>>) -> BoxFuture<'a, ApplicationResult<()>>,
>(
    stream: impl Stream<Item = Header>,
    f: T,
    current: Arc<Mutex<Value>>,
) -> ApplicationResult<()> {
    pin_mut!(stream);
    while let Some(header) = stream.next().await {
        f(header, current.clone()).await?;
    }
    Ok(())
}

async fn trigger_from_stream<
    L1Client: ITaikoL1Client,
    Taiko: ITaikoClient,
    TimeProvider: ITimeProvider,
>(
    stream: impl Stream<Item = SubSlot>,
    block_builder: Arc<Mutex<Preconfer<L1Client, Taiko, TimeProvider>>>,
    active_operator_model: Arc<Mutex<ActiveOperatorModel>>,
    handover_timeout: Duration,
) -> ApplicationResult<()> {
    pin_mut!(stream);
    loop {
        if let Some(subslot) = stream.next().await {
            trace!("Received subslot: {:?}", subslot);
            let epoch = if active_operator_model
                .lock()
                .await
                .within_handover_period(subslot.slot.slot)
            {
                subslot.slot.epoch + 1
            } else {
                subslot.slot.epoch
            };
            info!("Set active epoch to {} for slot {:?}", epoch, subslot);
            active_operator_model
                .lock()
                .await
                .set_next_active_epoch(epoch);
            if active_operator_model
                .lock()
                .await
                .can_preconfirm(&subslot.slot)
            {
                if active_operator_model
                    .lock()
                    .await
                    .is_first_preconfirmation_slot(&subslot.slot)
                {
                    trace!("First slot in window: {:?}", subslot.slot);
                    let monitor = DummySequencingMonitor {};
                    end_of_handover_start_buffer(handover_timeout, &monitor).await;
                    debug!("Last preconfer is done and l2 header is in sync");
                }
                if let Err(err) = block_builder.lock().await.build_block().await {
                    error!("Error during block building: {:?}", err.to_string())
                }
            } else {
                info!("Not active operator. Skip block building.");
            }
        }
    }
}

fn create_subslot_stream(config: &Config) -> ApplicationResult<impl Stream<Item = SubSlot>> {
    let taiko_slot_model = SlotModel::new(
        HOLESKY_GENESIS_TIMESTAMP,
        config.l2_slot_time,
        Duration::from_secs(384),
    );

    let start = get_next_slot_start(&config.l2_slot_time)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let taiko_slot = taiko_slot_model.get_slot(timestamp);

    let subslots_per_slot = config.l1_slot_time.as_secs() / config.l2_slot_time.as_secs();
    let slots_per_epoch = config.l1_slots_per_epoch * subslots_per_slot;
    let next_slot_count = taiko_slot.epoch * slots_per_epoch + taiko_slot.slot + 1;
    Ok(get_subslot_stream(
        get_slot_stream(start, next_slot_count, config.l2_slot_time, slots_per_epoch)?,
        subslots_per_slot,
    ))
}

async fn create_header_stream(
    client_url: &str,
    ws_url: &str,
) -> ApplicationResult<impl Stream<Item = Header>> {
    let l2_client = RpcClient::new(get_client(client_url)?);
    let polling_stream = get_polling_stream(l2_client, Duration::from_millis(100));

    let ws = WsConnect::new(ws_url);
    let provider = ProviderBuilder::new().connect_ws(ws).await?;
    let subscription = provider.subscribe_blocks().await?;
    let ws_stream = subscription.into_stream().map(|header| header.inner);
    Ok(get_header_stream(polling_stream, ws_stream))
}

async fn run_preconfer() -> ApplicationResult<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    dotenv::dotenv()?;
    let config = Config::try_from_env()?;

    let slot_stream = create_subslot_stream(&config)?;

    let l2_client = RpcClient::new(get_client(&config.l2_client_url)?);
    let jwt_secret =
        JwtSecret::from_hex("654c8ed1da58823433eb6285234435ed52418fa9141548bca1403cc0ad519432")
            .unwrap();
    let auth_client = RpcClient::new(get_auth_client(&config.l2_auth_client_url, jwt_secret)?);
    let l1_provider = ProviderBuilder::new()
        .connect(&config.l1_client_url)
        .await?;
    let l1_client = TaikoL1Client::new(
        RpcClient::new(get_client(&config.l1_client_url)?),
        l1_provider,
    );

    let taiko_anchor_address = get_taiko_anchor_address();
    let provider = ProviderBuilder::new()
        .connect(&config.l2_client_url)
        .await?;
    let taiko_anchor = TaikoAnchorInstance::new(taiko_anchor_address, provider.clone());

    let taiko = TaikoClient::new(
        l2_client,
        auth_client,
        taiko_anchor,
        provider,
        get_basefee_config_v2(),
        CHAIN_ID,
    );
    let handover_slots = config.handover_window_slots as u64;
    let block_builder = Arc::new(Mutex::new(Preconfer::new(
        config.anchor_id_lag,
        l1_client,
        taiko,
        Address::random(),
        SystemTimeProvider::new(),
    )));
    let shared_last_l1_block_number = block_builder.lock().await.shared_last_l1_block_number();
    let shared_parent_header = block_builder.lock().await.shared_parent_header();

    let process_l1_header = {
        |header: Header, current: Arc<Mutex<u64>>| {
            async move {
                let num = header.number;

                info!("L1 🗣 #{:<10} {}", num, header.timestamp);
                info!("{:?}", HOLESKY_SLOT_MODEL.get_slot(header.timestamp));
                *current.lock().await = header.number;
                Ok(())
            }
            .boxed()
        }
    };

    let process_l2_header = {
        |header: Header, current: Arc<Mutex<Option<Header>>>| {
            async move {
                let num = header.number;

                info!("L2 🗣 #{:<10} {}", num, header.timestamp);
                info!("{:?}", HOLESKY_SLOT_MODEL.get_slot(header.timestamp));
                *current.lock().await = Some(header);
                Ok(())
            }
            .boxed()
        }
    };

    let slots_per_epoch = 32;
    let active_operator_model = Arc::new(Mutex::new(ActiveOperatorModel::new(
        handover_slots,
        slots_per_epoch,
    )));

    let l1_header_stream = create_header_stream(&config.l1_client_url, &config.l1_ws_url).await?;
    let l2_header_stream = create_header_stream(&config.l2_client_url, &config.l2_ws_url).await?;
    let _ = join!(
        stream_block_headers_into(
            l1_header_stream,
            process_l1_header,
            shared_last_l1_block_number
        ),
        stream_block_headers_into(l2_header_stream, process_l2_header, shared_parent_header),
        trigger_from_stream(
            slot_stream,
            block_builder,
            active_operator_model,
            config.handover_start_buffer
        ),
    );

    Ok(())
}

#[tokio::main]
async fn main() -> ApplicationResult<()> {
    run_preconfer().await
}
