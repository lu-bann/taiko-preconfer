use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use alloy_primitives::Address;
use preconfirmation::{
    preconf::{
        BlockBuilder,
        config::Config,
        sequencing_monitor::{TaikoSequencingMonitor, TaikoStatusMonitor},
        slot_model::SlotModel as PreconfirmationSlotModel,
    },
    slot_model::SlotModel,
    taiko::{
        anchor::ValidAnchor, contracts::TaikoWhitelistInstance, taiko_l2_client::ITaikoL2Client,
    },
    time_provider::{ITimeProvider, SystemTimeProvider},
    util::{log_error, remaining_until_next_slot},
};
use tracing::{info, instrument};

use crate::{error::ApplicationResult, util::WhitelistMonitor};

#[instrument(name = "📋", skip_all)]
#[allow(clippy::too_many_arguments)]
pub async fn run<L2Client: ITaikoL2Client, TimeProvider: ITimeProvider>(
    builder: BlockBuilder<L2Client, TimeProvider>,
    slot_model: SlotModel,
    preconfirmation_slot_model: PreconfirmationSlotModel,
    whitelist: TaikoWhitelistInstance,
    sequencing_monitor: TaikoSequencingMonitor<TaikoStatusMonitor>,
    valid_anchor: ValidAnchor,
    waiting_for_previous_preconfer: Arc<AtomicBool>,
    preconfer_address: Address,
    config: Config,
) -> ApplicationResult<()> {
    let mut valid_anchor = valid_anchor;
    let mut preconfirmation_slot_model = preconfirmation_slot_model;
    let mut whitelist_monitor = WhitelistMonitor::new(whitelist);
    let provider = SystemTimeProvider::new();

    let subslots_per_slot = slot_model.slot_duration.as_secs() / config.l2_slot_duration.as_secs();
    let slot = slot_model.get_slot(provider.timestamp_in_s());
    whitelist_monitor.update_current_operator(slot.epoch).await;
    whitelist_monitor.update_next_operator(slot.epoch).await;
    if whitelist_monitor.is_active_in(preconfer_address, slot.epoch) {
        info!("Can preconfirm in current epoch on startup");
        preconfirmation_slot_model.set_active_epoch(slot.epoch);
    }
    if whitelist_monitor.is_active_in(preconfer_address, slot.epoch + 1) {
        info!("Can preconfirm in next epoch on startup");
        preconfirmation_slot_model.set_active_epoch(slot.epoch + 1);
    }

    tokio::time::sleep(remaining_until_next_slot(
        &config.l2_slot_duration,
        &provider,
    )?)
    .await;

    loop {
        let timestamp = provider.timestamp_in_s();
        let slot = slot_model.get_slot(timestamp);
        let slot_number = slot_model.get_slot_number(slot);
        let slot_start = slot_model.get_timestamp(slot_number);
        let sub =
            (timestamp - slot_start) / (slot_model.slot_duration.as_secs() / subslots_per_slot);
        let subslot = slot.slot * subslots_per_slot + sub;
        info!("Current slot {:?} {} {}", slot, sub, subslot,);
        info!("slot {slot_number} {slot_start} {timestamp}");
        info!("📩 Received subslot: {:?}", subslot);
        if waiting_for_previous_preconfer.load(Ordering::Relaxed) {
            info!("Waiting for previous preconfer to finish.");
            continue;
        }
        let slot_timestamp = slot_start + sub * config.l2_slot_duration.as_secs();
        info!(
            "slot number: L1={}, L2={}, L2 time={}",
            slot.epoch * 32 + slot.slot,
            slot.epoch * 64 + subslot,
            slot_timestamp,
        );
        if subslot == 0 {
            info!("Change active epoch to {}", slot.epoch);
            whitelist_monitor.change_epoch(slot.epoch);
        }
        if preconfirmation_slot_model.is_handover_start_slot(slot.slot)
            && sub == 0
            && whitelist_monitor.is_active_in(preconfer_address, slot.epoch + 1)
        {
            preconfirmation_slot_model.set_active_epoch(slot.epoch + 1);
        }

        if preconfirmation_slot_model.can_preconfirm(&slot)
            || whitelist_monitor.is_current_and_next(preconfer_address)
        {
            let shifted_slot = slot.slot + preconfirmation_slot_model.handover_slots();
            if shifted_slot % config.anchor_id_lag == 0 && sub == 0 {
                if shifted_slot == 0 {
                    valid_anchor.update_to_latest().await?;
                } else {
                    valid_anchor.update().await?;
                }
            }
            if preconfirmation_slot_model.is_first_preconfirmation_slot(&slot) && subslot == 0 {
                let handover_timeout = config.handover_start_buffer;
                let sequencing_monitor = sequencing_monitor.clone();
                waiting_for_previous_preconfer.store(true, Ordering::Relaxed);
                let waiting_for_previous_preconfer_spawn = waiting_for_previous_preconfer.clone();
                tokio::spawn(async move {
                    if log_error(
                        tokio::time::timeout(handover_timeout, sequencing_monitor.ready()).await,
                        "❌ Out of sync after handover period",
                    )
                    .is_some()
                    {
                        info!("✅ In sync after handover period");
                    }
                    waiting_for_previous_preconfer_spawn.store(false, Ordering::Relaxed);
                });
            }
            if waiting_for_previous_preconfer.load(Ordering::Relaxed) {
                let slot_timestamp_ms = slot_timestamp as u128 * 1000;
                let mut skip_slot = true;
                while provider.timestamp_in_ms()
                    < slot_timestamp_ms + config.status_sync_max_delay.as_millis()
                {
                    skip_slot = waiting_for_previous_preconfer.load(Ordering::Relaxed);
                    tokio::time::sleep(config.poll_period).await;
                }
                if skip_slot {
                    continue;
                }
            }

            let end_of_sequencing = preconfirmation_slot_model
                .is_last_slot_before_handover_window(slot.slot)
                && subslot % subslots_per_slot == subslots_per_slot - 1;
            log_error(
                tokio::time::timeout(
                    Duration::from_millis(1500),
                    builder.build_block(slot_timestamp, end_of_sequencing),
                )
                .await,
                "Error building block",
            );
        } else {
            info!("Not active operator. Skip block building.");
        }

        if preconfirmation_slot_model.is_last_slot_before_handover_window(slot.slot) {
            whitelist_monitor.update_next_operator(slot.epoch).await;
        }

        tokio::time::sleep(remaining_until_next_slot(
            &config.l2_slot_duration,
            &provider,
        )?)
        .await;
    }
}
