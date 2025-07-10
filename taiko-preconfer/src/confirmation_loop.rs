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
    slot_model::SlotModel,
    taiko::{
        anchor::ValidAnchor, contracts::TaikoWhitelistInstance, taiko_l1_client::ITaikoL1Client,
    },
    time_provider::{ITimeProvider, SystemTimeProvider},
    util::{log_error, remaining_until_next_slot},
};
use tracing::{debug, info, instrument};

use crate::{error::ApplicationResult, util::WhitelistMonitor};

#[instrument(name = "➡️:", skip_all)]
#[allow(clippy::too_many_arguments)]
pub async fn run<L1Client: ITaikoL1Client>(
    confirmation_strategy: BlockConstrainedConfirmationStrategy<L1Client>,
    slot_model: SlotModel,
    preconfirmation_slot_model: PreconfirmationSlotModel,
    whitelist: TaikoWhitelistInstance,
    preconfer_address: Address,
    valid_anchor: ValidAnchor,
    waiting_for_previous_preconfer: Arc<AtomicBool>,
    shared_latest_l1_timestamp: Arc<AtomicU64>,
) -> ApplicationResult<()> {
    let mut preconfirmation_slot_model = preconfirmation_slot_model;
    let mut whitelist_monitor = WhitelistMonitor::new(whitelist);
    let provider = SystemTimeProvider::new();

    let slot = slot_model.get_slot(provider.timestamp_in_s());
    whitelist_monitor.update_current_operator(slot.epoch).await;
    whitelist_monitor.update_next_operator(slot.epoch).await;
    if whitelist_monitor.is_active_in(preconfer_address, slot.epoch) {
        info!("Can confirm in current epoch on startup");
        preconfirmation_slot_model.set_active_epoch(slot.epoch);
    }
    if whitelist_monitor.is_active_in(preconfer_address, slot.epoch + 1) {
        info!("Can confirm in next epoch on startup");
        preconfirmation_slot_model.set_active_epoch(slot.epoch + 1);
    }

    tokio::time::sleep(remaining_until_next_slot(
        &slot_model.slot_duration,
        &provider,
    )?)
    .await;
    loop {
        let timestamp = provider.timestamp_in_s();
        let slot = slot_model.get_slot(timestamp);
        info!("📩 Current slot: {:?}", slot);
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

        debug!(
            "L1 slot timestamp: {} {}",
            slot_model.get_timestamp(&slot),
            provider.timestamp_in_s(),
        );

        if (preconfirmation_slot_model.can_confirm(&slot)
            || whitelist_monitor.is_current_and_next(preconfer_address))
            && !preconfirmation_slot_model.is_last_slot_before_handover_window(slot.slot)
        {
            let force_send = preconfirmation_slot_model.within_handover_period(slot.slot);

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
