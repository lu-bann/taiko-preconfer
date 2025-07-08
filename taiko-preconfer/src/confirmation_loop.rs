use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use alloy_primitives::Address;
use preconfirmation::{
    preconf::{
        confirmation_strategy::BlockConstrainedConfirmationStrategy,
        slot_model::SlotModel as PreconfirmationSlotModel,
    },
    slot::Slot,
    slot_model::SlotModel,
    taiko::{
        anchor::ValidAnchor, contracts::TaikoWhitelistInstance, taiko_l1_client::ITaikoL1Client,
    },
    time_provider::SystemTimeProvider,
    util::{get_system_time_from_s, log_error, now_as_secs, remaining_until_next_slot},
};
use tracing::{info, instrument};

use crate::{error::ApplicationResult, util::WhitelistMonitor};

#[instrument(name = "➡️:", skip_all)]
#[allow(clippy::too_many_arguments)]
pub async fn run<L1Client: ITaikoL1Client>(
    confirmation_strategy: BlockConstrainedConfirmationStrategy<L1Client>,
    preconfirmation_slot_model: PreconfirmationSlotModel,
    whitelist: TaikoWhitelistInstance,
    preconfer_address: Address,
    valid_anchor: ValidAnchor,
    waiting_for_previous_preconfer: Arc<AtomicBool>,
    shared_latest_l1_timestamp: Arc<AtomicU64>,
) -> ApplicationResult<()> {
    let mut preconfirmation_slot_model = preconfirmation_slot_model;
    let slot_model = SlotModel::holesky();
    let mut whitelist_monitor = WhitelistMonitor::new(whitelist);

    let slot = slot_model.get_slot(now_as_secs());
    whitelist_monitor.update_current_operator(slot.epoch).await;
    whitelist_monitor.update_next_operator(slot.epoch).await;
    if whitelist_monitor.is_active_in(preconfer_address, slot.epoch) {
        info!("Can confirm on startup");
        preconfirmation_slot_model.set_active_epoch(slot.epoch);
    }

    let provider = SystemTimeProvider::new();
    tokio::time::sleep(remaining_until_next_slot(
        &slot_model.slot_duration,
        &provider,
    )?)
    .await;
    loop {
        let timestamp = now_as_secs();
        let slot = slot_model.get_slot(timestamp);
        info!("📩 Received slot: {:?}", slot);
        if slot.slot == 0 {
            whitelist_monitor.change_epoch(slot.epoch);
        }
        if preconfirmation_slot_model.is_handover_start_slot(slot.slot)
            && whitelist_monitor.is_active_in(preconfer_address, slot.epoch + 1)
        {
            preconfirmation_slot_model.set_active_epoch(slot.epoch + 1);
        }

        if waiting_for_previous_preconfer.load(Ordering::Relaxed) {
            info!("Waiting for previous preconfer to finish.");
            continue;
        }

        let total_slot = slot.epoch * 32 + slot.slot;
        let l1_slot_timestamp = slot_model.get_timestamp(total_slot);
        info!(
            "L1 slot timestamp: {} {}",
            l1_slot_timestamp,
            preconfirmation::util::now_as_secs(),
        );

        if preconfirmation_slot_model.can_confirm(&slot)
            || whitelist_monitor.is_current_and_next(preconfer_address)
        {
            let mut force_send = preconfirmation_slot_model.within_handover_period(slot.slot);
            if !force_send && slot.slot > 16 {
                let handover_start_slot = Slot::new(slot.epoch, 28);
                let total_slot = slot_model.get_slot_number(handover_start_slot);
                let handover_start_timestamp =
                    get_system_time_from_s(slot_model.get_timestamp(total_slot));
                force_send = !valid_anchor.is_valid_at(handover_start_timestamp).await;
            }
            let current_anchor_id = valid_anchor.id_and_state_root().await.0;
            log_error(
                confirmation_strategy
                    .send(
                        shared_latest_l1_timestamp.load(Ordering::Relaxed)
                            + slot_model.slot_duration.as_secs(),
                        force_send,
                        current_anchor_id,
                    )
                    .await,
                "Failed to send blocks",
            );
        }

        if preconfirmation_slot_model.is_last_slot_before_handover_window(slot.slot) {
            whitelist_monitor.update_next_operator(slot.epoch).await;
        }

        tokio::time::sleep(remaining_until_next_slot(
            &slot_model.slot_duration,
            &provider,
        )?)
        .await;
    }
}
