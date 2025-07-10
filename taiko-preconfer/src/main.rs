use std::{
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64},
    },
    time::UNIX_EPOCH,
};

use alloy_consensus::Transaction;
use alloy_eips::eip4844::env_settings::EnvKzgSettings;
use alloy_network::EthereumWallet;
use alloy_primitives::Address;
use alloy_provider::{Provider, ProviderBuilder, WsConnect};
use alloy_signer::k256::ecdsa::SigningKey;
use alloy_signer_local::{LocalSigner, PrivateKeySigner};
use preconfirmation::{
    client::reqwest::get_block_by_id,
    preconf::{
        BlockBuilder,
        config::Config,
        confirmation_strategy::BlockConstrainedConfirmationStrategy,
        sequencing_monitor::{TaikoSequencingMonitor, TaikoStatusMonitor},
        slot_model::SlotModel as PreconfirmationSlotModel,
    },
    slot_model::SlotModel,
    taiko::{
        anchor::ValidAnchor,
        contracts::{
            BaseFeeConfig, TaikoAnchorInstance, TaikoInboxInstance, TaikoWhitelistInstance,
        },
        sign::get_signing_key,
        taiko_l1_client::{ITaikoL1Client, TaikoL1Client},
        taiko_l2_client::{ITaikoL2Client, TaikoL2Client},
    },
    time_provider::SystemTimeProvider,
    tx_cache::TxCache,
    util::get_anchor_block_id_from_bytes,
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

async fn get_taiko_l2_client(
    config: &Config,
    base_fee_config: &BaseFeeConfig,
    preconfer_address: Address,
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
        preconfer_address,
    ))
}

async fn get_taiko_l1_client(
    config: &Config,
    preconfer_address: Address,
) -> ApplicationResult<TaikoL1Client> {
    let signer = PrivateKeySigner::from_str(&config.private_key.read())?;
    let wallet = EthereumWallet::from(signer);

    let l1_provider = ProviderBuilder::new()
        .wallet(wallet)
        .connect(&config.l1_client_url)
        .await?;

    let taiko_inbox = get_inbox(config).await?;
    Ok(TaikoL1Client::new(
        l1_provider,
        config.propose_timeout,
        preconfer_address,
        config.taiko_preconf_router_address,
        taiko_inbox,
        config.use_blobs,
        config.relative_fee_premium,
        config.relative_blob_fee_premium,
    ))
}

async fn get_whitelist(config: &Config) -> ApplicationResult<TaikoWhitelistInstance> {
    let l1_provider = ProviderBuilder::new()
        .connect(&config.l1_client_url)
        .await?;
    Ok(TaikoWhitelistInstance::new(
        config.taiko_whitelist_address,
        l1_provider,
    ))
}

async fn get_inbox(config: &Config) -> ApplicationResult<TaikoInboxInstance> {
    let l1_ws_provider = ProviderBuilder::new()
        .connect_ws(WsConnect::new(&config.l1_ws_url))
        .await?;
    Ok(TaikoInboxInstance::new(
        config.taiko_inbox_address,
        l1_ws_provider,
    ))
}

#[tokio::main]
#[allow(clippy::result_large_err)]
async fn main() -> ApplicationResult<()> {
    info!("Starting preconfer");
    dotenv::dotenv()?;
    let config = Config::try_from_env()?;

    // initialize kzg settings before using it later as this can take about 2s
    let _ = EnvKzgSettings::Default.get();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let genesis_timestamp = config
        .l1_genesis_time
        .duration_since(UNIX_EPOCH)
        .expect("Genesis must be after epoch")
        .as_secs();
    let slot_model = SlotModel::new(
        genesis_timestamp,
        config.l1_slot_duration,
        config.l1_slot_duration * config.l1_slots_per_epoch as u32,
    );
    let taiko_inbox = get_inbox(&config).await?;
    let taiko_inbox_config = taiko_inbox.pacayaConfig().call().await?;

    let signer =
        LocalSigner::<SigningKey>::from_signing_key(get_signing_key(&config.private_key.read()));
    let preconfer_address = signer.address();
    info!("Preconfer address: {}", preconfer_address);

    let taiko_l2_client = get_taiko_l2_client(
        &config,
        &taiko_inbox_config.baseFeeConfig,
        preconfer_address,
    )
    .await?;
    let latest_l2_header = taiko_l2_client.get_latest_header().await?;
    let latest_l2_header_number = latest_l2_header.number;

    let shared_last_l2_header = Arc::new(RwLock::new(latest_l2_header));
    let taiko_l1_client = get_taiko_l1_client(&config, preconfer_address).await?;
    let latest_l1_header = taiko_l1_client.get_latest_header().await?;
    let shared_latest_l1_timestamp = Arc::new(AtomicU64::new(latest_l1_header.timestamp));
    let whitelist = get_whitelist(&config).await?;

    let preconfirmation_url = config.l2_preconfirmation_url.clone() + "/status";
    let taiko_sequencing_monitor = TaikoSequencingMonitor::new(
        shared_last_l2_header.clone(),
        config.poll_period,
        TaikoStatusMonitor::new(preconfirmation_url),
    );

    let max_anchor_id_offset = taiko_inbox_config.maxAnchorHeightOffset;
    let mut valid_anchor = ValidAnchor::new(
        max_anchor_id_offset,
        config.anchor_id_lag,
        config.l1_client_url.clone(),
    );

    let latest_l1_header = taiko_l1_client.get_latest_header().await?;
    valid_anchor.update_block_number(latest_l1_header.number);
    let block_builder = BlockBuilder::new(
        valid_anchor.clone(),
        taiko_l2_client,
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
    valid_anchor.update_last_anchor_id(latest_confirmed_batch.anchorBlockId);
    valid_anchor.update().await?;

    let mut unconfirmed_l2_blocks = vec![];
    for block_id in (latest_confirmed_block_id + 1)..=latest_l2_header_number {
        let block = get_block_by_id(config.l2_client_url.clone(), Some(block_id)).await?;
        if !block.transactions.is_empty() && block.header.beneficiary == preconfer_address {
            let anchor_id = get_anchor_block_id_from_bytes(
                block
                    .transactions
                    .txns()
                    .next()
                    .expect("Must be present")
                    .input(),
            )?;
            if anchor_id >= latest_confirmed_batch.anchorBlockId {
                unconfirmed_l2_blocks.push(block);
            }
        }
    }
    info!(
        "Picked up {} unconfirmed blocks at startup",
        unconfirmed_l2_blocks.len()
    );

    let tx_cache = TxCache::new(unconfirmed_l2_blocks);
    let valid_timestamp = preconfirmation::util::ValidTimestamp::new(
        max_anchor_id_offset * config.l1_slot_duration.as_secs(),
    );

    let confirmation_strategy = BlockConstrainedConfirmationStrategy::new(
        taiko_l1_client.clone(),
        tx_cache.clone(),
        config.max_blocks_per_batch,
        valid_timestamp,
    );

    let l1_header_stream =
        create_header_stream(&config.l1_client_url, &config.l1_ws_url, config.poll_period).await?;

    let l2_header_stream = create_l2_head_stream(
        &config,
        latest_confirmed_block_id,
        tx_cache,
        valid_anchor.clone(),
        taiko_inbox,
    )
    .await?;

    let waiting_for_previous_preconfer = Arc::new(AtomicBool::new(false));
    let (onchain_tracking_result, preconfirmation_result, confirmation_result) = join!(
        onchain_tracking_loop::run(
            l1_header_stream,
            l2_header_stream,
            shared_last_l2_header,
            valid_anchor.clone(),
            shared_latest_l1_timestamp.clone(),
        ),
        preconfirmation_loop::run(
            block_builder,
            slot_model,
            preconfirmation_slot_model.clone(),
            whitelist.clone(),
            taiko_sequencing_monitor,
            valid_anchor.clone(),
            waiting_for_previous_preconfer.clone(),
            preconfer_address,
            config.clone(),
        ),
        confirmation_loop::run(
            confirmation_strategy,
            slot_model,
            preconfirmation_slot_model,
            whitelist,
            preconfer_address,
            valid_anchor,
            waiting_for_previous_preconfer,
            shared_latest_l1_timestamp,
        )
    );

    onchain_tracking_result?;
    preconfirmation_result?;
    confirmation_result
}
