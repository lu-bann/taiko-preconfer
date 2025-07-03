use alloy_primitives::Address;
use preconfirmation::{preconf::slot_model::SlotModel as PreconfirmationSlotModel, slot::Slot};
use tracing::info;

pub fn set_active_operator_if_necessary(
    current_preconfer: &Address,
    preconfer_address: &Address,
    preconfirmation_slot_model: &mut PreconfirmationSlotModel,
    slot: &Slot,
) {
    if !preconfirmation_slot_model.can_preconfirm(slot) && current_preconfer == preconfer_address {
        preconfirmation_slot_model.set_active_epoch(slot.epoch);
        info!("Set active epoch to {} for slot {:?}", slot.epoch, slot);
    }
}

pub fn set_active_operator_for_next_period(
    next_preconfer: &Address,
    preconfer_address: &Address,
    preconfirmation_slot_model: &mut PreconfirmationSlotModel,
    slot: &Slot,
) {
    info!("Preconfer for next epoch: {}", next_preconfer);
    if next_preconfer == preconfer_address {
        preconfirmation_slot_model.set_active_epoch(slot.epoch + 1);
        info!("Set active epoch to {} for slot {:?}", slot.epoch + 1, slot);
    }
}
