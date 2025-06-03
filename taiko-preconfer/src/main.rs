use alloy_consensus::Header;
use alloy_primitives::Address;
use alloy_provider::{Provider, ProviderBuilder, WsConnect};
use alloy_rpc_types_engine::JwtSecret;
use futures::{Stream, StreamExt, future::BoxFuture, pin_mut};
use preconfirmation::{
    active_operator_model::ActiveOperatorModel,
    client::{RpcClient, get_alloy_auth_client, get_alloy_client},
    preconf::{
        Preconfer,
        config::Config,
        handover_start_buffer::{DummySequencingMonitor, end_of_handover_start_buffer},
    },
    slot::SubSlot,
    slot_model::{HOLESKY_GENESIS_TIMESTAMP, HOLESKY_SLOT_MODEL, SlotModel},
    stream::{
        get_header_stream, get_next_slot_start, get_polling_stream, get_slot_stream,
        get_subslot_stream, stream_headers, to_boxed,
    },
    taiko::{
        contracts::{TaikoAnchorInstance, TaikoWhitelistInstance},
        hekla::{
            CHAIN_ID,
            addresses::{get_golden_touch_address, get_taiko_anchor_address},
            get_basefee_config_v2,
        },
        taiko_client::{ITaikoClient, TaikoClient},
        taiko_l1_client::{ITaikoL1Client, TaikoL1Client},
    },
    time_provider::{ITimeProvider, SystemTimeProvider},
};
use std::{str::FromStr, time::Duration};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{join, sync::Mutex};
use tracing::{debug, error, info, trace};

mod error;
use crate::error::ApplicationResult;

const DUMMY_WHITELIST: &str = "0x90A309073b5F2f7C821e0aAc68b2c6F42F649c59";
const BASE_SEPOLIA_RPC: &str = "https://sepolia.base.org";

async fn trigger_from_stream<
    L1Client: ITaikoL1Client,
    Taiko: ITaikoClient,
    TimeProvider: ITimeProvider,
>(
    stream: impl Stream<Item = SubSlot>,
    preconfer: Arc<Mutex<Preconfer<L1Client, Taiko, TimeProvider>>>,
    active_operator_model: Arc<Mutex<ActiveOperatorModel>>,
    handover_timeout: Duration,
) -> ApplicationResult<()> {
    pin_mut!(stream);
    loop {
        if let Some(subslot) = stream.next().await {
            info!("Received subslot: {:?}", subslot);
            if !active_operator_model
                .lock()
                .await
                .can_preconfirm(&subslot.slot)
                && preconfer
                    .lock()
                    .await
                    .l1_client()
                    .get_current_preconfer()
                    .await?
                    == get_golden_touch_address()
            {
                let epoch = if active_operator_model
                    .lock()
                    .await
                    .within_handover_period(subslot.slot.slot)
                {
                    subslot.slot.epoch + 1
                } else {
                    subslot.slot.epoch
                };
                active_operator_model
                    .lock()
                    .await
                    .set_next_active_epoch(epoch);
                info!("Set active epoch to {} for slot {:?}", epoch, subslot);
            }
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
                if let Err(err) = preconfer.lock().await.build_block().await {
                    error!("Error during block building: {:?}", err.to_string())
                }
            } else {
                info!("Not active operator. Skip block building.");
            }
            if active_operator_model
                .lock()
                .await
                .is_last_slot_before_handover_window(subslot.slot.slot)
            {
                let next_preconfer = preconfer
                    .lock()
                    .await
                    .l1_client()
                    .get_preconfer_for_next_epoch()
                    .await?;
                info!(" *** Preconfer for next epoch: {} ***", next_preconfer);
                if next_preconfer == get_golden_touch_address() {
                    active_operator_model
                        .lock()
                        .await
                        .set_next_active_epoch(subslot.slot.epoch + 1);
                    info!(
                        "Set active epoch to {} for slot {:?}",
                        subslot.slot.epoch + 1,
                        subslot
                    );
                }
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

    let time_provider = SystemTimeProvider::new();
    let start = get_next_slot_start(&config.l2_slot_time, &time_provider)?;
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
    let l2_client = RpcClient::new(get_alloy_client(client_url, false)?);
    let polling_stream = get_polling_stream(l2_client, Duration::from_millis(100));

    let ws = WsConnect::new(ws_url);
    let provider = ProviderBuilder::new().connect_ws(ws).await?;
    let subscription = provider.subscribe_blocks().await?;
    let ws_stream = subscription.into_stream().map(|header| header.inner);
    Ok(get_header_stream(polling_stream, ws_stream))
}

fn get_config() -> ApplicationResult<Config> {
    dotenv::dotenv()?;
    Ok(Config::try_from_env()?)
}

async fn get_taiko_l2_client(config: &Config) -> ApplicationResult<TaikoClient> {
    let l2_client = RpcClient::new(get_alloy_client(&config.l2_client_url, false)?);
    let jwt_secret =
        JwtSecret::from_hex("654c8ed1da58823433eb6285234435ed52418fa9141548bca1403cc0ad519432")
            .unwrap();
    let auth_client = RpcClient::new(get_alloy_auth_client(
        &config.l2_auth_client_url,
        jwt_secret,
        true,
    )?);

    let taiko_anchor_address = get_taiko_anchor_address();
    let provider = ProviderBuilder::new()
        .connect(&config.l2_client_url)
        .await?;
    let taiko_anchor = TaikoAnchorInstance::new(taiko_anchor_address, provider.clone());

    Ok(TaikoClient::new(
        l2_client,
        auth_client,
        taiko_anchor,
        provider,
        get_basefee_config_v2(),
        CHAIN_ID,
    ))
}

async fn get_taiko_l1_client(config: &Config) -> ApplicationResult<TaikoL1Client> {
    let l1_provider = ProviderBuilder::new()
        .connect(&config.l1_client_url)
        .await?;
    let l1_provider_base = ProviderBuilder::new().connect(BASE_SEPOLIA_RPC).await?;
    let whitelist = TaikoWhitelistInstance::new(
        Address::from_str(DUMMY_WHITELIST).unwrap(),
        l1_provider_base,
    );
    Ok(TaikoL1Client::new(
        RpcClient::new(get_alloy_client(&config.l1_client_url, false)?),
        l1_provider,
        whitelist,
    ))
}

async fn store_header_number(header: Header, current: Arc<Mutex<u64>>) -> ApplicationResult<()> {
    info!("L1 🗣 #{:<10} {}", header.number, header.timestamp);
    info!("{:?}", HOLESKY_SLOT_MODEL.get_slot(header.timestamp));
    *current.lock().await = header.number;
    Ok(())
}

async fn store_header(
    header: Header,
    current: Arc<Mutex<Option<Header>>>,
) -> ApplicationResult<()> {
    info!("L2 🗣 #{:<10} {}", header.number, header.timestamp);
    info!("{:?}", HOLESKY_SLOT_MODEL.get_slot(header.timestamp));
    *current.lock().await = Some(header);
    Ok(())
}

fn store_header_boxed<'a>(
    header: Header,
    current: Arc<Mutex<Option<Header>>>,
) -> BoxFuture<'a, ApplicationResult<()>> {
    to_boxed(header, current, store_header)
}

fn store_header_number_boxed<'a>(
    header: Header,
    current: Arc<Mutex<u64>>,
) -> BoxFuture<'a, ApplicationResult<()>> {
    to_boxed(header, current, store_header_number)
}

#[tokio::main]
async fn main() -> ApplicationResult<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    let config = get_config()?;

    let taiko_l2_client = get_taiko_l2_client(&config).await?;
    let taiko_l1_client = get_taiko_l1_client(&config).await?;

    let preconfer = Arc::new(Mutex::new(Preconfer::new(
        config.anchor_id_lag,
        taiko_l1_client,
        taiko_l2_client,
        Address::random(),
        SystemTimeProvider::new(),
    )));

    let slots_per_epoch = 32;
    let handover_slots = config.handover_window_slots as u64;
    let active_operator_model = Arc::new(Mutex::new(ActiveOperatorModel::new(
        handover_slots,
        slots_per_epoch,
    )));

    let slot_stream = create_subslot_stream(&config)?;
    let l1_header_stream = create_header_stream(&config.l1_client_url, &config.l1_ws_url).await?;
    let l2_header_stream = create_header_stream(&config.l2_client_url, &config.l2_ws_url).await?;

    let shared_last_l1_block_number = preconfer.lock().await.shared_last_l1_block_number();
    let shared_parent_header = preconfer.lock().await.shared_parent_header();

    let _ = join!(
        stream_headers(
            l1_header_stream,
            store_header_number_boxed,
            shared_last_l1_block_number
        ),
        stream_headers(l2_header_stream, store_header_boxed, shared_parent_header),
        trigger_from_stream(
            slot_stream,
            preconfer,
            active_operator_model,
            config.handover_start_buffer
        ),
    );

    Ok(())
}
