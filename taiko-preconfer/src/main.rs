use alloy_consensus::Header;
use alloy_primitives::Address;
use alloy_provider::{Provider, ProviderBuilder, WsConnect};
use alloy_rpc_types_engine::JwtSecret;
use alloy_signer::k256::ecdsa::SigningKey;
use alloy_signer_local::LocalSigner;
use futures::{Stream, StreamExt, future::BoxFuture, pin_mut};
use preconfirmation::{
    active_operator_model::ActiveOperatorModel,
    client::{RpcClient, get_alloy_auth_client, get_alloy_client},
    preconf::{
        Preconfer,
        config::Config,
        confirmation_strategy::InstantConfirmationStrategy,
        handover_start_buffer::{TaikoSequencingMonitor, TaikoStatusMonitor},
    },
    slot::SubSlot,
    slot_model::SlotModel,
    stream::{
        get_header_stream, get_next_slot_start, get_polling_stream, get_slot_stream,
        get_subslot_stream, stream_headers, to_boxed,
    },
    taiko::{
        contracts::{TaikoAnchorInstance, TaikoWhitelistInstance},
        hekla::get_basefee_config_v2,
        sign::get_signing_key,
        taiko_l1_client::{ITaikoL1Client, TaikoL1Client},
        taiko_l2_client::{ITaikoL2Client, TaikoL2Client},
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

fn log_error<T, E: ToString>(result: Result<T, E>, msg: &str) -> Option<T> {
    match result {
        Err(err) => {
            error!("{msg}: {}", err.to_string());
            None
        }
        Ok(value) => Some(value),
    }
}

fn set_active_operator_if_necessary(
    current_preconfer: &Address,
    preconfer_address: &Address,
    active_operator_model: &mut ActiveOperatorModel,
    subslot: &SubSlot,
) {
    if !active_operator_model.can_preconfirm(&subslot.slot)
        && current_preconfer == preconfer_address
    {
        let epoch = if active_operator_model.within_handover_period(subslot.slot.slot) {
            subslot.slot.epoch + 1
        } else {
            subslot.slot.epoch
        };
        active_operator_model.set_next_active_epoch(epoch);
        info!("Set active epoch to {} for slot {:?}", epoch, subslot);
    }
}

fn set_active_operator_for_next_period(
    next_preconfer: &Address,
    preconfer_address: &Address,
    active_operator_model: &mut ActiveOperatorModel,
    subslot: &SubSlot,
) {
    info!(" *** Preconfer for next epoch: {} ***", next_preconfer);
    if next_preconfer == preconfer_address {
        active_operator_model.set_next_active_epoch(subslot.slot.epoch + 1);
        info!(
            "Set active epoch to {} for slot {:?}",
            subslot.slot.epoch + 1,
            subslot
        );
    }
}

async fn trigger_from_stream<
    L1Client: ITaikoL1Client,
    L2Client: ITaikoL2Client,
    TimeProvider: ITimeProvider,
>(
    stream: impl Stream<Item = SubSlot>,
    preconfer: Preconfer<L1Client, L2Client, TimeProvider>,
    active_operator_model: ActiveOperatorModel,
    sequencing_monitor: TaikoSequencingMonitor<TaikoStatusMonitor>,
    whitelist: TaikoWhitelistInstance,
    confirmation_strategy: InstantConfirmationStrategy<L1Client>,
    handover_timeout: Duration,
) -> ApplicationResult<()> {
    let mut active_operator_model = active_operator_model;
    pin_mut!(stream);
    let preconfer_address = preconfer.address();

    loop {
        if let Some(subslot) = stream.next().await {
            info!("Received subslot: {:?}", subslot);

            if let Some(current_preconfer) = log_error(
                whitelist.getOperatorForCurrentEpoch().call().await,
                "Failed to read current preconfer",
            ) {
                set_active_operator_if_necessary(
                    &current_preconfer,
                    &preconfer_address,
                    &mut active_operator_model,
                    &subslot,
                );
            }

            if active_operator_model.can_preconfirm(&subslot.slot) {
                if active_operator_model.is_first_preconfirmation_slot(&subslot.slot) {
                    trace!("First slot in window: {:?}", subslot.slot);
                    if log_error(
                        tokio::time::timeout(handover_timeout, sequencing_monitor.ready()).await,
                        "State out of sync after handover period",
                    )
                    .is_some()
                    {
                        debug!("Last preconfer is done and l2 header is in sync");
                    }
                }

                if let Some(result) =
                    log_error(preconfer.build_block().await, "Error building block")
                {
                    match result {
                        None => info!("No block created in slot {:?}", subslot),
                        Some(block) => {
                            log_error(
                                confirmation_strategy.confirm(block).await,
                                "Failed to confirm block",
                            );
                        }
                    }
                }
            } else {
                info!("Not active operator. Skip block building.");
            }

            if active_operator_model.is_last_slot_before_handover_window(subslot.slot.slot) {
                if let Some(next_preconfer) = log_error(
                    whitelist.getOperatorForNextEpoch().call().await,
                    "Failed to read preconfer for next epoch",
                ) {
                    set_active_operator_for_next_period(
                        &next_preconfer,
                        &preconfer_address,
                        &mut active_operator_model,
                        &subslot,
                    );
                }
            }
        }
    }
}

fn create_subslot_stream(config: &Config) -> ApplicationResult<impl Stream<Item = SubSlot>> {
    let taiko_slot_model = SlotModel::taiko_holesky(config.l2_slot_time);

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
    poll_period: Duration,
) -> ApplicationResult<impl Stream<Item = Header>> {
    let client = get_alloy_client(client_url, false)?;
    let polling_stream = get_polling_stream(client, poll_period);

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

async fn get_taiko_l2_client(config: &Config) -> ApplicationResult<TaikoL2Client> {
    let jwt_secret =
        JwtSecret::from_hex("654c8ed1da58823433eb6285234435ed52418fa9141548bca1403cc0ad519432")
            .unwrap();
    let auth_client = RpcClient::new(get_alloy_auth_client(
        &config.l2_auth_client_url,
        jwt_secret,
        true,
    )?);

    let taiko_anchor_address = Address::from_str(&config.taiko_anchor_address)?;
    let provider = ProviderBuilder::new()
        .connect(&config.l2_client_url)
        .await?;
    let taiko_anchor = TaikoAnchorInstance::new(taiko_anchor_address, provider.clone());

    let chain_id = provider.get_chain_id().await?;
    trace!("L2 chain id {chain_id}");
    Ok(TaikoL2Client::new(
        auth_client,
        taiko_anchor,
        provider,
        get_basefee_config_v2(),
        chain_id,
        get_signing_key(&config.golden_touch_private_key),
    ))
}

async fn get_taiko_l1_client(config: &Config) -> ApplicationResult<TaikoL1Client> {
    let l1_provider = ProviderBuilder::new()
        .connect(&config.l1_client_url)
        .await?;
    let chain_id = l1_provider.get_chain_id().await?;
    Ok(TaikoL1Client::new(l1_provider, chain_id))
}

async fn store_header_number(header: Header, current: Arc<Mutex<u64>>) -> ApplicationResult<()> {
    info!("L1 🗣 #{:<10} {}", header.number, header.timestamp);
    info!("{:?}", SlotModel::holesky().get_slot(header.timestamp));
    *current.lock().await = header.number;
    Ok(())
}

async fn store_header(
    header: Header,
    current: Arc<Mutex<Option<Header>>>,
) -> ApplicationResult<()> {
    info!("L2 🗣 #{:<10} {}", header.number, header.timestamp);
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

    let signer =
        LocalSigner::<SigningKey>::from_signing_key(get_signing_key(&config.private_key.read()));
    let shared_header = Arc::new(Mutex::new(None));
    let taiko_l2_client = get_taiko_l2_client(&config).await?;
    let taiko_l1_client = get_taiko_l1_client(&config).await?;
    let l1_provider = ProviderBuilder::new()
        .connect(&config.l1_client_url)
        .await?;
    let whitelist = TaikoWhitelistInstance::new(
        Address::from_str(&config.taiko_whitelist_address).unwrap(),
        l1_provider,
    );

    let preconfirmation_url = config.l2_preconfirmation_url.clone() + "/status";
    let taiko_sequencing_monitor = TaikoSequencingMonitor::new(
        shared_header.clone(),
        config.poll_period,
        TaikoStatusMonitor::new(preconfirmation_url),
    );
    let l1_chain_id = taiko_l1_client.chain_id();

    let shared_last_l1_block_number = Arc::new(Mutex::new(0u64));
    let preconfer_address = signer.address();
    let preconfer = Preconfer::new(
        config.anchor_id_lag,
        taiko_l1_client,
        taiko_l2_client,
        preconfer_address,
        SystemTimeProvider::new(),
        shared_last_l1_block_number.clone(),
        shared_header.clone(),
        config.golden_touch_address.clone(),
    );

    let slots_per_epoch = 32;
    let handover_slots = config.handover_window_slots as u64;
    let active_operator_model = ActiveOperatorModel::new(handover_slots, slots_per_epoch);

    let taiko_l1_client = get_taiko_l1_client(&config).await?;
    let confirmation_strategy = InstantConfirmationStrategy::new(
        taiko_l1_client,
        preconfer.address(),
        l1_chain_id,
        Address::from_str(&config.taiko_inbox_address)?,
        signer,
    );

    let slot_stream = create_subslot_stream(&config)?;
    let l1_header_stream =
        create_header_stream(&config.l1_client_url, &config.l1_ws_url, config.poll_period).await?;
    let l2_header_stream =
        create_header_stream(&config.l2_client_url, &config.l2_ws_url, config.poll_period).await?;

    let _ = join!(
        stream_headers(
            l1_header_stream,
            store_header_number_boxed,
            shared_last_l1_block_number
        ),
        stream_headers(l2_header_stream, store_header_boxed, shared_header),
        trigger_from_stream(
            slot_stream,
            preconfer,
            active_operator_model,
            taiko_sequencing_monitor,
            whitelist,
            confirmation_strategy,
            config.handover_start_buffer
        ),
    );

    Ok(())
}
