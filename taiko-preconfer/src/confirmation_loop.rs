use alloy_primitives::Address;
use futures::{Stream, StreamExt, pin_mut};
use preconfirmation::{
    preconf::{
        confirmation_strategy::BlockConstrainedConfirmationStrategy,
        slot_model::SlotModel as PreconfirmationSlotModel,
    },
    slot::Slot,
    slot_model::SlotModel,
    taiko::{contracts::TaikoWhitelistInstance, taiko_l1_client::ITaikoL1Client},
    util::{log_error, now_as_secs},
};
use tracing::{info, instrument};

use crate::{
    error::ApplicationResult,
    util::{set_active_operator_for_next_period, set_active_operator_if_necessary},
};

#[instrument(name = "➡️", skip_all)]
pub async fn run<L1Client: ITaikoL1Client>(
    stream: impl Stream<Item = Slot>,
    confirmation_strategy: BlockConstrainedConfirmationStrategy<L1Client>,
    preconfirmation_slot_model: PreconfirmationSlotModel,
    whitelist: TaikoWhitelistInstance,
    preconfer_address: Address,
) -> ApplicationResult<()> {
    let mut preconfirmation_slot_model = preconfirmation_slot_model;
    pin_mut!(stream);
    let slot_model = SlotModel::holesky();
    let mut current_epoch_preconfer = Address::ZERO;
    let mut next_epoch_preconfer = Address::ZERO;

    if let Some(current_preconfer) = log_error(
        whitelist.getOperatorForCurrentEpoch().call().await,
        "Failed to read current preconfer",
    ) {
        let slot_model = SlotModel::holesky();
        let current_slot = slot_model.get_slot(now_as_secs());
        info!("Whitelist current preconfer: {current_preconfer}");
        set_active_operator_if_necessary(
            &current_preconfer,
            &preconfer_address,
            &mut preconfirmation_slot_model,
            &current_slot,
        );
        current_epoch_preconfer = current_preconfer;
    }

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
            let l1_slot_timestamp = slot_model.get_timestamp(total_slot);
            info!(
                "L1 slot timestamp: {} {}",
                l1_slot_timestamp,
                preconfirmation::util::now_as_secs(),
            );

            info!(
                "Current preconfer: {current_epoch_preconfer}, next preconfer: {next_epoch_preconfer}"
            );

            if can_confirm
                || (current_epoch_preconfer == preconfer_address
                    && next_epoch_preconfer == preconfer_address)
            {
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
                    next_epoch_preconfer = next_preconfer;
                }
            }
        }
    }
}
