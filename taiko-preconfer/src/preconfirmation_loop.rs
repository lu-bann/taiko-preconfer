use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use preconfirmation::{
    preconf::{
        BlockBuilder,
        sequencing_monitor::{TaikoSequencingMonitor, TaikoStatusMonitor},
        slot_model::SlotModel as PreconfirmationSlotModel,
    },
    slot::SubSlot,
    slot_model::{HOLESKY_GENESIS_TIMESTAMP, SlotModel},
    taiko::{contracts::TaikoWhitelistInstance, taiko_l2_client::ITaikoL2Client},
    time_provider::{ITimeProvider, SystemTimeProvider},
    util::{log_error, now_as_millis, now_as_secs, remaining_until_next_slot},
};
use tracing::{info, instrument};

use crate::{error::ApplicationResult, util::WhitelistMonitor};

#[instrument(name = "📋", skip_all)]
#[allow(clippy::too_many_arguments)]
pub async fn run<L2Client: ITaikoL2Client, TimeProvider: ITimeProvider>(
    builder: BlockBuilder<L2Client, TimeProvider>,
    preconfirmation_slot_model: PreconfirmationSlotModel,
    whitelist: TaikoWhitelistInstance,
    sequencing_monitor: TaikoSequencingMonitor<TaikoStatusMonitor>,
    handover_timeout: Duration,
    l2_slot_duration: Duration,
    waiting_for_previous_preconfer: Arc<AtomicBool>,
    polling_period: Duration,
    status_sync_max_delay: Duration,
) -> ApplicationResult<()> {
    let mut preconfirmation_slot_model = preconfirmation_slot_model;
    let preconfer_address = builder.address();
    let mut whitelist_monitor = WhitelistMonitor::new(whitelist);

    let slot_model = SlotModel::holesky();
    let subslots_per_slot = slot_model.slot_duration.as_secs() / l2_slot_duration.as_secs();
    let slot = slot_model.get_slot(now_as_secs());
    whitelist_monitor.update_current_operator(slot.epoch).await;
    whitelist_monitor.update_next_operator(slot.epoch).await;
    if whitelist_monitor.is_active_in(preconfer_address, slot.epoch) {
        info!("Active on startup");
        preconfirmation_slot_model.set_active_epoch(slot.epoch);
    }

    let provider = SystemTimeProvider::new();
    tokio::time::sleep(remaining_until_next_slot(&l2_slot_duration, &provider)?).await;

    loop {
        let timestamp = now_as_secs();
        let slot = slot_model.get_slot(timestamp);
        let slot_number = slot_model.get_slot_number(slot);
        let slot_start = slot_model.get_timestamp(slot_number);
        let sub =
            (timestamp - slot_start) / (slot_model.slot_duration.as_secs() / subslots_per_slot);
        info!(
            "Current slot {:?} {} {}",
            slot,
            sub,
            slot.slot * subslots_per_slot + sub
        );
        info!("slot {slot_number} {slot_start} {timestamp}");
        let subslot = SubSlot::new(slot, slot.slot * subslots_per_slot + sub);
        info!("📩 Received subslot: {:?}", subslot);
        if waiting_for_previous_preconfer.load(Ordering::Relaxed) {
            info!("Waiting for previous preconfer to finish.");
            continue;
        }
        let slot_timestamp =
            HOLESKY_GENESIS_TIMESTAMP + subslot.slot.epoch * 32 * 12 + subslot.sub_slot * 6;
        info!(
            "slot number: L1={}, L2={}, L2 time={}, now={}",
            subslot.slot.epoch * 32 + subslot.slot.slot,
            subslot.slot.epoch * 64 + subslot.sub_slot,
            slot_timestamp,
            now_as_secs(),
        );
        if slot_timestamp + 10 < now_as_secs() {
            panic!("Out of sync");
        }
        whitelist_monitor.change_epoch(slot.epoch);

        let is_last_slot_before_handover_window =
            preconfirmation_slot_model.is_last_slot_before_handover_window(subslot.slot.slot);
        if preconfirmation_slot_model.can_preconfirm(&subslot.slot)
            || whitelist_monitor.is_current_and_next(preconfer_address)
        {
            if preconfirmation_slot_model.is_first_preconfirmation_slot(&subslot.slot)
                && subslot.sub_slot == 0
            {
                let handover_timeout = handover_timeout;
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
                while now_as_millis() < slot_timestamp_ms + status_sync_max_delay.as_millis() {
                    skip_slot = waiting_for_previous_preconfer.load(Ordering::Relaxed);
                    tokio::time::sleep(polling_period).await;
                }
                if skip_slot {
                    continue;
                }
            }

            let end_of_sequencing = is_last_slot_before_handover_window
                && subslot.sub_slot % subslots_per_slot == subslots_per_slot - 1;
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

        if preconfirmation_slot_model.is_last_slot_before_handover_window(subslot.slot.slot) {
            whitelist_monitor.update_next_operator(slot.epoch).await;
            if whitelist_monitor.is_active_in(preconfer_address, slot.epoch + 1) {
                preconfirmation_slot_model.set_active_epoch(slot.epoch + 1);
            }
        }

        tokio::time::sleep(remaining_until_next_slot(&l2_slot_duration, &provider)?).await;
    }
}
