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

use crate::{
    error::ApplicationResult,
    util::{set_active_operator_for_next_period, set_active_operator_if_necessary},
};

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
    let mut current_epoch_preconfer = Address::ZERO;
    let mut next_epoch_preconfer = Address::ZERO;

    let current_slot = slot_model.get_slot(now_as_secs());
    if let Some(current_preconfer) = log_error(
        whitelist.getOperatorForCurrentEpoch().call().await,
        "Failed to read current preconfer",
    ) {
        set_active_operator_if_necessary(
            &current_preconfer,
            &preconfer_address,
            &mut preconfirmation_slot_model,
            &current_slot,
        );
        current_epoch_preconfer = current_preconfer;
    }
    if let Some(next_preconfer) = log_error(
        whitelist.getOperatorForNextEpoch().call().await,
        "Failed to read preconfer for next epoch",
    ) {
        set_active_operator_for_next_period(
            &next_preconfer,
            &preconfer_address,
            &mut preconfirmation_slot_model,
            &current_slot,
        );
        next_epoch_preconfer = next_preconfer;
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

        info!(
            "Current preconfer: {current_epoch_preconfer}, next preconfer: {next_epoch_preconfer}"
        );

        if preconfirmation_slot_model.can_confirm(&slot)
            || (current_epoch_preconfer == preconfer_address
                && next_epoch_preconfer == preconfer_address)
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
                next_epoch_preconfer = next_preconfer;
            }
        }
        tokio::time::sleep(remaining_until_next_slot(
            &slot_model.slot_duration,
            &provider,
        )?)
        .await;
    }
}
