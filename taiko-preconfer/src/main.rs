use std::{str::FromStr, sync::Arc, time::Duration};

use alloy_consensus::Header;
use alloy_network::EthereumWallet;
use alloy_primitives::Address;
use alloy_provider::{Provider, ProviderBuilder, WsConnect};
use alloy_rpc_types_engine::JwtSecret;
use alloy_rpc_types_eth::Block;
use alloy_signer::k256::ecdsa::SigningKey;
use alloy_signer_local::{LocalSigner, PrivateKeySigner};
use futures::{Stream, StreamExt, future::BoxFuture, pin_mut};
use preconfirmation::{
    client::{
        RpcClient, get_alloy_auth_client, get_alloy_client,
        get_block_by_id as alloy_get_block_by_id, reqwest::get_block_by_id,
    },
    preconf::{
        BlockBuilder,
        config::Config,
        confirmation_strategy::{BlockConstrainedConfirmationStrategy, ConfirmationSender},
        sequencing_monitor::{TaikoSequencingMonitor, TaikoStatusMonitor},
        slot_model::SlotModel as PreconfirmationSlotModel,
    },
    slot::{Slot, SubSlot},
    slot_model::{HOLESKY_GENESIS_TIMESTAMP, SlotModel},
    stream::{
        get_block_polling_stream, get_block_stream, get_confirmed_id_polling_stream,
        get_header_polling_stream, get_header_stream, get_id_stream, get_l2_head_stream,
        get_next_slot_start, get_slot_stream, get_subslot_stream, stream_l2_headers, to_boxed,
    },
    taiko::{
        anchor::ValidAnchorId,
        contracts::{TaikoAnchorInstance, TaikoInboxInstance, TaikoWhitelistInstance},
        hekla::get_basefee_config_v2,
        sign::get_signing_key,
        taiko_l1_client::{ITaikoL1Client, TaikoL1Client},
        taiko_l2_client::{ITaikoL2Client, TaikoL2Client},
    },
    time_provider::{ITimeProvider, SystemTimeProvider},
    util::{log_error, now_as_secs},
    verification::{LastBatchVerifier, get_latest_confirmed_batch},
};
use tokio::{join, sync::RwLock};
use tracing::{debug, info, trace};

mod error;
use crate::error::ApplicationResult;

fn set_active_operator_if_necessary(
    current_preconfer: &Address,
    preconfer_address: &Address,
    preconfirmation_slot_model: &mut PreconfirmationSlotModel,
    slot: &Slot,
) {
    if !preconfirmation_slot_model.can_preconfirm(slot) && current_preconfer == preconfer_address {
        let epoch = if preconfirmation_slot_model.within_handover_period(slot.slot) {
            slot.epoch + 1
        } else {
            slot.epoch
        };
        preconfirmation_slot_model.set_next_active_epoch(epoch);
        info!("Set active epoch to {} for slot {:?}", epoch, slot);
    }
}

fn set_active_operator_for_next_period(
    next_preconfer: &Address,
    preconfer_address: &Address,
    preconfirmation_slot_model: &mut PreconfirmationSlotModel,
    slot: &Slot,
) {
    info!(" *** Preconfer for next epoch: {} ***", next_preconfer);
    if next_preconfer == preconfer_address {
        preconfirmation_slot_model.set_next_active_epoch(slot.epoch + 1);
        info!("Set active epoch to {} for slot {:?}", slot.epoch + 1, slot);
    }
}

async fn stream_l1_headers<
    'a,
    E,
    T: Fn(Header, Arc<RwLock<ValidAnchorId>>) -> BoxFuture<'a, Result<(), E>>,
>(
    stream: impl Stream<Item = Header>,
    f: T,
    valid_anchor_id: Arc<RwLock<ValidAnchorId>>,
) -> Result<(), E> {
    pin_mut!(stream);
    while let Some(header) = stream.next().await {
        f(header, valid_anchor_id.clone()).await?;
    }
    Ok(())
}

async fn preconfirmation_loop<
    L1Client: ITaikoL1Client,
    L2Client: ITaikoL2Client,
    TimeProvider: ITimeProvider,
>(
    stream: impl Stream<Item = SubSlot>,
    builder: BlockBuilder<L1Client, L2Client, TimeProvider>,
    preconfirmation_slot_model: PreconfirmationSlotModel,
    sequencing_monitor: TaikoSequencingMonitor<TaikoStatusMonitor>,
    whitelist: TaikoWhitelistInstance,
    handover_timeout: Duration,
) -> ApplicationResult<()> {
    let mut preconfirmation_slot_model = preconfirmation_slot_model;
    pin_mut!(stream);
    let preconfer_address = builder.address();

    loop {
        if let Some(subslot) = stream.next().await {
            info!("Received subslot: {:?}", subslot);
            info!(
                "L1 slot number {}",
                subslot.slot.epoch * 32 + subslot.slot.slot
            );
            info!(
                "L2 slot number {}",
                subslot.slot.epoch * 64 + subslot.sub_slot
            );
            let slot_timestamp =
                HOLESKY_GENESIS_TIMESTAMP + subslot.slot.epoch * 32 * 12 + subslot.sub_slot * 6 + 6;
            info!(
                "L2 slot timestamp {} {}",
                slot_timestamp,
                preconfirmation::util::now_as_secs()
            );

            if let Some(current_preconfer) = log_error(
                whitelist.getOperatorForCurrentEpoch().call().await,
                "Failed to read current preconfer",
            ) {
                set_active_operator_if_necessary(
                    &current_preconfer,
                    &preconfer_address,
                    &mut preconfirmation_slot_model,
                    &subslot.slot,
                );
            }

            if preconfirmation_slot_model.can_preconfirm(&subslot.slot) {
                if preconfirmation_slot_model.is_first_preconfirmation_slot(&subslot.slot) {
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

                log_error(builder.build_block().await, "Error building block");
            } else {
                info!("Not active operator. Skip block building.");
            }

            if preconfirmation_slot_model.is_last_slot_before_handover_window(subslot.slot.slot) {
                if let Some(next_preconfer) = log_error(
                    whitelist.getOperatorForNextEpoch().call().await,
                    "Failed to read preconfer for next epoch",
                ) {
                    set_active_operator_for_next_period(
                        &next_preconfer,
                        &preconfer_address,
                        &mut preconfirmation_slot_model,
                        &subslot.slot,
                    );
                }
            }
        }
    }
}

async fn confirmation_loop<L1Client: ITaikoL1Client>(
    stream: impl Stream<Item = Slot>,
    preconfirmation_slot_model: PreconfirmationSlotModel,
    whitelist: TaikoWhitelistInstance,
    confirmation_strategy: BlockConstrainedConfirmationStrategy<L1Client>,
    preconfer_address: Address,
) -> ApplicationResult<()> {
    let mut preconfirmation_slot_model = preconfirmation_slot_model;
    pin_mut!(stream);
    let slot_model = SlotModel::holesky();

    loop {
        if let Some(slot) = stream.next().await {
            info!("Received slot: {:?}", slot);
            let total_slot = slot.epoch * 32 + slot.slot;
            info!("Total slot: {}", total_slot);
            let can_confirm = preconfirmation_slot_model.can_confirm(&slot);
            let can_preconfirm = preconfirmation_slot_model.can_preconfirm(&slot);
            let within_handover_period =
                preconfirmation_slot_model.within_handover_period(slot.slot);
            let last_slot_before_handover_window =
                preconfirmation_slot_model.is_last_slot_before_handover_window(slot.slot);
            info!("Can confirm: {}", can_confirm);
            info!("Can preconfirm: {}", can_preconfirm);
            info!("within handover period: {}", within_handover_period);
            let l1_slot_timestamp = slot_model.get_timestamp(total_slot + 1);
            info!(
                "L1 slot timestamp: {} {} {}",
                total_slot * 12 + preconfirmation::slot_model::HOLESKY_GENESIS_TIMESTAMP,
                l1_slot_timestamp,
                preconfirmation::util::now_as_secs(),
            );

            if let Some(current_preconfer) = log_error(
                whitelist.getOperatorForCurrentEpoch().call().await,
                "Failed to read current preconfer",
            ) {
                set_active_operator_if_necessary(
                    &current_preconfer,
                    &preconfer_address,
                    &mut preconfirmation_slot_model,
                    &slot,
                );
            }

            if true {
                let force_send = within_handover_period;
                log_error(
                    confirmation_strategy
                        .send(l1_slot_timestamp, force_send)
                        .await,
                    "Failed to send blocks",
                );
            }

            if last_slot_before_handover_window {
                if let Some(next_preconfer) = log_error(
                    whitelist.getOperatorForNextEpoch().call().await,
                    "Failed to read preconfer for next epoch",
                ) {
                    set_active_operator_for_next_period(
                        &next_preconfer,
                        &preconfer_address,
                        &mut preconfirmation_slot_model,
                        &slot,
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
    let timestamp = time_provider.timestamp_in_s();
    let taiko_slot = taiko_slot_model.get_slot(timestamp);

    let subslots_per_slot = config.l1_slot_time.as_secs() / config.l2_slot_time.as_secs();
    let slot_number = taiko_slot_model.get_slot_number(taiko_slot);
    Ok(get_subslot_stream(
        get_slot_stream(
            start,
            slot_number,
            config.l2_slot_time,
            taiko_slot_model.slots_per_epoch(),
        )?,
        subslots_per_slot,
    ))
}

fn create_slot_stream(config: &Config) -> ApplicationResult<impl Stream<Item = Slot>> {
    let taiko_slot_model = SlotModel::holesky();

    let time_provider = SystemTimeProvider::new();
    let start = get_next_slot_start(&config.l1_slot_time, &time_provider)?;
    let timestamp = time_provider.timestamp_in_s();
    let taiko_slot = taiko_slot_model.get_slot(timestamp);
    let slot_number = taiko_slot_model.get_slot_number(taiko_slot);

    Ok(get_slot_stream(
        start,
        slot_number,
        config.l1_slot_time,
        config.l1_slots_per_epoch,
    )?)
}

async fn create_header_stream(
    client_url: &str,
    ws_url: &str,
    poll_period: Duration,
) -> ApplicationResult<impl Stream<Item = Header>> {
    let client = get_alloy_client(client_url, false)?;
    let polling_stream = get_header_polling_stream(client, poll_period);

    let ws = WsConnect::new(ws_url);
    let provider = ProviderBuilder::new().connect_ws(ws).await?;
    let subscription = provider.subscribe_blocks().await?;
    let ws_stream = subscription.into_stream().map(|header| header.inner);
    Ok(get_header_stream(polling_stream, ws_stream))
}

async fn create_block_stream(
    client_url: &str,
    ws_url: &str,
    poll_period: Duration,
) -> ApplicationResult<impl Stream<Item = Block>> {
    let client = get_alloy_client(client_url, false)?;
    let full_tx = true;
    let polling_stream = get_block_polling_stream(client, poll_period, full_tx);

    let ws = WsConnect::new(ws_url);
    let provider = ProviderBuilder::new().connect_ws(ws).await?;
    let ws_stream = provider
        .subscribe_full_blocks()
        .into_stream()
        .await?
        .filter_map(|res| async move { res.ok() });
    Ok(get_block_stream(polling_stream, ws_stream))
}

fn get_config() -> ApplicationResult<Config> {
    dotenv::dotenv()?;
    Ok(Config::try_from_env()?)
}

async fn get_taiko_l2_client(config: &Config) -> ApplicationResult<TaikoL2Client> {
    let jwt_secret = JwtSecret::from_hex(config.jwt_secret.read()).unwrap();
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
    let preconfirmation_url = config.l2_preconfirmation_url.clone() + "/preconfBlocks";
    Ok(TaikoL2Client::new(
        auth_client,
        taiko_anchor,
        provider,
        get_basefee_config_v2(),
        chain_id,
        get_signing_key(&config.golden_touch_private_key),
        preconfirmation_url,
        config.jwt_secret.clone(),
    ))
}

async fn get_taiko_l1_client(config: &Config) -> ApplicationResult<TaikoL1Client> {
    let signer = PrivateKeySigner::from_str(&config.private_key.read())?;
    let wallet = EthereumWallet::from(signer);

    let l1_provider = ProviderBuilder::new()
        .wallet(wallet)
        .connect(&config.l1_client_url)
        .await?;
    let chain_id = l1_provider.get_chain_id().await?;
    Ok(TaikoL1Client::new(l1_provider, chain_id))
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
    valid_anchor_id: Arc<RwLock<ValidAnchorId>>,
) -> ApplicationResult<()> {
    info!(
        "L1 🗣 #{:<10} timestamp={} now={} state_root={:?} gas_used={}",
        header.number,
        header.timestamp,
        now_as_secs(),
        header.state_root,
        header.gas_used
    );
    valid_anchor_id
        .write()
        .await
        .update_block_number(header.number);
    Ok(())
}

async fn store_header_l2(header: Header, current: Arc<RwLock<Header>>) -> ApplicationResult<()> {
    store_header(header, current, "L2".to_string()).await
}

fn store_valid_anchor_boxed<'a>(
    header: Header,
    valid_anchor_id: Arc<RwLock<ValidAnchorId>>,
) -> BoxFuture<'a, ApplicationResult<()>> {
    Box::pin(store_valid_anchor(header, valid_anchor_id))
}

fn store_header_boxed_l2<'a>(
    header: Header,
    current: Arc<RwLock<Header>>,
) -> BoxFuture<'a, ApplicationResult<()>> {
    to_boxed(header, current, store_header_l2)
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
    let taiko_l2_client = get_taiko_l2_client(&config).await?;
    let latest_l2_header = taiko_l2_client.get_latest_header().await?;
    let latest_l2_header_number = latest_l2_header.number;

    let shared_last_l2_header = Arc::new(RwLock::new(latest_l2_header));
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
        shared_last_l2_header.clone(),
        config.poll_period,
        TaikoStatusMonitor::new(preconfirmation_url),
    );

    let l1_provider = ProviderBuilder::new()
        .connect_ws(WsConnect::new(&config.l1_ws_url))
        .await?;
    let taiko_inbox = TaikoInboxInstance::new(
        Address::from_str(&config.taiko_inbox_address).unwrap(),
        l1_provider.clone(),
    );

    let max_anchor_id_offset = taiko_inbox
        .pacayaConfig()
        .call()
        .await
        .unwrap()
        .maxAnchorHeightOffset;
    let valid_anchor_id = Arc::new(RwLock::new(ValidAnchorId::new(
        max_anchor_id_offset,
        config.anchor_id_lag,
        config.anchor_id_update_tol,
    )));

    let latest_l1_header = taiko_l1_client.get_latest_header().await?;
    valid_anchor_id
        .write()
        .await
        .update_block_number(latest_l1_header.number);
    let preconfer_address = signer.address();
    info!("Preconfer address: {}", preconfer_address);
    let block_builder = BlockBuilder::new(
        valid_anchor_id.clone(),
        taiko_l1_client.clone(),
        taiko_l2_client,
        preconfer_address,
        SystemTimeProvider::new(),
        shared_last_l2_header.clone(),
        config.golden_touch_address.clone(),
    );

    let handover_slots = config.handover_window_slots as u64;
    let preconfirmation_slot_model =
        PreconfirmationSlotModel::new(handover_slots, config.l1_slots_per_epoch);

    let latest_confirmed_batch = get_latest_confirmed_batch(&taiko_inbox).await?;
    let latest_confirmed_block_id = latest_confirmed_batch.lastBlockId;
    valid_anchor_id
        .write()
        .await
        .update_last_anchor_id(latest_confirmed_batch.anchorBlockId);
    valid_anchor_id.write().await.update();

    let mut unconfirmed_l2_blocks = vec![];
    let l2_client = get_alloy_client(&config.l2_client_url, false)?;
    for block_id in (latest_confirmed_block_id + 1)..=latest_l2_header_number {
        let block = alloy_get_block_by_id(&l2_client, block_id, true).await?;
        unconfirmed_l2_blocks.push(block);
    }
    info!(
        "Picked up {} unconfirmed blocks at startup",
        unconfirmed_l2_blocks.len()
    );
    let unconfirmed_l2_blocks = Arc::new(RwLock::new(unconfirmed_l2_blocks));
    let max_blocks = 2;
    let valid_timestamp = preconfirmation::util::ValidTimestamp::new(
        max_anchor_id_offset * config.l1_slot_time.as_secs(),
    );

    let confirmation_strategy = BlockConstrainedConfirmationStrategy::new(
        ConfirmationSender::new(
            taiko_l1_client.clone(),
            Address::from_str(&config.taiko_preconf_router_address)?,
            signer,
        ),
        unconfirmed_l2_blocks.clone(),
        max_blocks,
        valid_anchor_id.clone(),
        taiko_inbox.clone(),
        valid_timestamp,
    );

    let subslot_stream = create_subslot_stream(&config)?;
    let slot_stream = create_slot_stream(&config)?;
    let l1_header_stream =
        create_header_stream(&config.l1_client_url, &config.l1_ws_url, config.poll_period).await?;

    let l2_block_stream =
        create_block_stream(&config.l2_client_url, &config.l2_ws_url, config.poll_period).await?;
    let anchor_id_update_valid_anchor = valid_anchor_id.clone();
    let batch_proposed_stream = taiko_inbox
        .BatchProposed_filter()
        .subscribe()
        .await?
        .into_stream()
        .filter_map(|result| {
            let anchor_id_update_valid_anchor = anchor_id_update_valid_anchor.clone();
            async move {
                match result {
                    Ok((batch_proposed, _log)) => {
                        anchor_id_update_valid_anchor
                            .write()
                            .await
                            .update_last_anchor_id(batch_proposed.info.anchorBlockId);
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
    let last_batch_verifier = LastBatchVerifier::new(taiko_inbox, preconfer_address);
    let l2_header_stream = get_l2_head_stream(
        l2_block_stream,
        id_stream,
        unconfirmed_l2_blocks,
        get_header_call,
        Some(latest_confirmed_block_id),
        last_batch_verifier,
    )
    .await;

    let _ = join!(
        stream_l1_headers(l1_header_stream, store_valid_anchor_boxed, valid_anchor_id,),
        stream_l2_headers(
            l2_header_stream,
            store_header_boxed_l2,
            shared_last_l2_header
        ),
        preconfirmation_loop(
            subslot_stream,
            block_builder,
            preconfirmation_slot_model.clone(),
            taiko_sequencing_monitor,
            whitelist.clone(),
            config.handover_start_buffer
        ),
        confirmation_loop(
            slot_stream,
            preconfirmation_slot_model,
            whitelist,
            confirmation_strategy,
            preconfer_address,
        )
    );

    Ok(())
}
