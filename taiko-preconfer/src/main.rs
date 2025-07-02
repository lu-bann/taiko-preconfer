use std::{str::FromStr, sync::Arc};

use alloy_network::EthereumWallet;
use alloy_provider::{Provider, ProviderBuilder, WsConnect};
use alloy_signer::k256::ecdsa::SigningKey;
use alloy_signer_local::{LocalSigner, PrivateKeySigner};
use futures::Stream;
use preconfirmation::{
    client::reqwest::get_block_by_id,
    preconf::{
        BlockBuilder,
        config::Config,
        confirmation_strategy::{BlockConstrainedConfirmationStrategy, ConfirmationSender},
        sequencing_monitor::{TaikoSequencingMonitor, TaikoStatusMonitor},
        slot_model::SlotModel as PreconfirmationSlotModel,
    },
    slot::{Slot, SubSlot},
    slot_model::SlotModel,
    stream::{get_next_slot_start, get_slot_stream, get_subslot_stream},
    taiko::{
        anchor::ValidAnchor,
        contracts::{
            BaseFeeConfig, TaikoAnchorInstance, TaikoInboxInstance, TaikoWhitelistInstance,
        },
        sign::get_signing_key,
        taiko_l1_client::{ITaikoL1Client, TaikoL1Client},
        taiko_l2_client::{ITaikoL2Client, TaikoL2Client},
    },
    time_provider::{ITimeProvider, SystemTimeProvider},
    verification::get_latest_confirmed_batch,
};
use tokio::{join, sync::RwLock};
use tracing::info;

use taiko_preconfer::{
    confirmation_loop,
    error::ApplicationResult,
    onchain_tracking_loop::{self, create_header_stream, create_l2_head_stream},
    preconfirmation_loop,
};

const PRECONF_BLOCKS: &str = "preconfBlocks";

#[allow(clippy::result_large_err)]
fn create_subslot_stream(config: &Config) -> ApplicationResult<impl Stream<Item = SubSlot>> {
    let taiko_slot_model = SlotModel::taiko_holesky(config.l2_slot_time);

    let time_provider = SystemTimeProvider::new();
    let start = get_next_slot_start(&config.l2_slot_time, &time_provider)?;
    let timestamp = time_provider.timestamp_in_s();
    let taiko_slot = taiko_slot_model.get_slot(timestamp);

    let subslots_per_slot = config.l1_slot_time.as_secs() / config.l2_slot_time.as_secs();
    let slot_number = taiko_slot_model.get_slot_number(taiko_slot) + 1;
    let l2_slot_in_epoch = slot_number % (config.l1_slots_per_epoch * subslots_per_slot);
    info!(
        "L2 slot on startup: {}, {} {} {:?}",
        slot_number,
        slot_number / subslots_per_slot,
        l2_slot_in_epoch,
        start,
    );
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

#[allow(clippy::result_large_err)]
fn create_slot_stream(config: &Config) -> ApplicationResult<impl Stream<Item = Slot>> {
    let taiko_slot_model = SlotModel::holesky();

    let time_provider = SystemTimeProvider::new();
    let start = get_next_slot_start(&config.l1_slot_time, &time_provider)?;
    let timestamp = time_provider.timestamp_in_s();
    let taiko_slot = taiko_slot_model.get_slot(timestamp);
    let slot_number = taiko_slot_model.get_slot_number(taiko_slot) + 1;
    info!("L1 slot on startup: {} {:?}", slot_number, start);

    Ok(get_slot_stream(
        start,
        slot_number,
        config.l1_slot_time,
        config.l1_slots_per_epoch,
    )?)
}

async fn get_taiko_l2_client(
    config: &Config,
    base_fee_config: &BaseFeeConfig,
) -> ApplicationResult<TaikoL2Client> {
    let provider = ProviderBuilder::new()
        .connect(&config.l2_client_url)
        .await?;
    let taiko_anchor = TaikoAnchorInstance::new(config.taiko_anchor_address, provider.clone());

    let chain_id = provider.get_chain_id().await?;
    let preconfirmation_url = config.l2_preconfirmation_url.clone() + "/" + PRECONF_BLOCKS;
    Ok(TaikoL2Client::new(
        config.l2_auth_client_url.clone(),
        taiko_anchor,
        provider,
        base_fee_config.clone(),
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
    Ok(TaikoL1Client::new(l1_provider, config.propose_timeout))
}

#[tokio::main]
#[allow(clippy::result_large_err)]
async fn main() -> ApplicationResult<()> {
    dotenv::dotenv()?;
    let config = Config::try_from_env()?;

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let l1_ws_provider = ProviderBuilder::new()
        .connect_ws(WsConnect::new(&config.l1_ws_url))
        .await?;
    let taiko_inbox = TaikoInboxInstance::new(config.taiko_inbox_address, l1_ws_provider.clone());

    let taiko_inbox_config = taiko_inbox.pacayaConfig().call().await?;

    let signer =
        LocalSigner::<SigningKey>::from_signing_key(get_signing_key(&config.private_key.read()));
    let taiko_l2_client = get_taiko_l2_client(&config, &taiko_inbox_config.baseFeeConfig).await?;
    let latest_l2_header = taiko_l2_client.get_latest_header().await?;
    let latest_l2_header_number = latest_l2_header.number;

    let shared_last_l2_header = Arc::new(RwLock::new(latest_l2_header));
    let taiko_l1_client = get_taiko_l1_client(&config).await?;
    let l1_provider = ProviderBuilder::new()
        .connect(&config.l1_client_url)
        .await?;
    let whitelist =
        TaikoWhitelistInstance::new(config.taiko_whitelist_address, l1_provider.clone());

    let preconfirmation_url = config.l2_preconfirmation_url.clone() + "/status";
    let taiko_sequencing_monitor = TaikoSequencingMonitor::new(
        shared_last_l2_header.clone(),
        config.poll_period,
        TaikoStatusMonitor::new(preconfirmation_url),
    );

    let max_anchor_id_offset = taiko_inbox_config.maxAnchorHeightOffset;
    let valid_anchor = Arc::new(RwLock::new(ValidAnchor::new(
        max_anchor_id_offset,
        config.anchor_id_lag,
        config.anchor_id_update_tol,
        taiko_l1_client.clone(),
    )));

    let latest_l1_header = taiko_l1_client.get_latest_header().await?;
    valid_anchor
        .write()
        .await
        .update_block_number(latest_l1_header.number)
        .await?;
    let preconfer_address = signer.address();
    info!("Preconfer address: {}", preconfer_address);
    let block_builder = BlockBuilder::new(
        valid_anchor.clone(),
        taiko_l2_client,
        preconfer_address,
        SystemTimeProvider::new(),
        shared_last_l2_header.clone(),
        config.golden_touch_address,
    );

    let preconfirmation_slot_model = PreconfirmationSlotModel::new(
        config.handover_window_slots as u64,
        config.l1_slots_per_epoch,
    );

    let latest_confirmed_batch = get_latest_confirmed_batch(&taiko_inbox).await?;
    let latest_confirmed_block_id = latest_confirmed_batch.lastBlockId;
    valid_anchor
        .write()
        .await
        .update_last_anchor_id(latest_confirmed_batch.anchorBlockId)
        .await?;

    let mut unconfirmed_l2_blocks = vec![];
    for block_id in (latest_confirmed_block_id + 1)..=latest_l2_header_number {
        let block = get_block_by_id(config.l2_client_url.clone(), Some(block_id)).await?;
        unconfirmed_l2_blocks.push(block);
    }
    info!(
        "Picked up {} unconfirmed blocks at startup",
        unconfirmed_l2_blocks.len()
    );
    let unconfirmed_l2_blocks = Arc::new(RwLock::new(unconfirmed_l2_blocks));
    let valid_timestamp = preconfirmation::util::ValidTimestamp::new(
        max_anchor_id_offset * config.l1_slot_time.as_secs(),
    );

    let confirmation_strategy = BlockConstrainedConfirmationStrategy::new(
        ConfirmationSender::new(
            taiko_l1_client.clone(),
            config.taiko_preconf_router_address,
            signer,
        ),
        unconfirmed_l2_blocks.clone(),
        config.max_blocks_per_batch,
        valid_anchor.clone(),
        taiko_inbox.clone(),
        valid_timestamp,
        config.use_blobs,
    );

    let l1_header_stream =
        create_header_stream(&config.l1_client_url, &config.l1_ws_url, config.poll_period).await?;

    let l2_header_stream = create_l2_head_stream(
        &config,
        preconfer_address,
        latest_confirmed_block_id,
        unconfirmed_l2_blocks,
        valid_anchor.clone(),
        taiko_inbox,
    )
    .await?;

    let subslot_stream = create_subslot_stream(&config)?;
    let slot_stream = create_slot_stream(&config)?;
    let subslots_per_slot = config.l1_slot_time.as_secs() / config.l2_slot_time.as_secs();
    let (onchain_tracking_result, preconfirmation_result, confirmation_result) = join!(
        onchain_tracking_loop::run(
            l1_header_stream,
            l2_header_stream,
            shared_last_l2_header,
            valid_anchor
        ),
        preconfirmation_loop::run(
            subslot_stream,
            block_builder,
            preconfirmation_slot_model.clone(),
            whitelist.clone(),
            taiko_sequencing_monitor,
            config.handover_start_buffer,
            subslots_per_slot,
        ),
        confirmation_loop::run(
            slot_stream,
            confirmation_strategy,
            preconfirmation_slot_model,
            whitelist,
            preconfer_address,
        )
    );

    onchain_tracking_result?;
    preconfirmation_result?;
    confirmation_result
}
