use crate::slot::Slot;

pub struct ActiveOperatorModel {
    next_active_epoch: Option<u64>,
    handover_slots: u64,
    slots_per_epoch: u64,
}

impl ActiveOperatorModel {
    pub const fn new(handover_slots: u64, slots_per_epoch: u64) -> Self {
        Self {
            next_active_epoch: None,
            handover_slots,
            slots_per_epoch,
        }
    }

    pub fn set_next_active_epoch(&mut self, epoch: u64) {
        self.next_active_epoch = Some(epoch);
    }

    pub fn within_handover_period(&self, slot: u64) -> bool {
        slot >= self.slots_per_epoch - self.handover_slots
    }

    pub fn can_preconfirm(&self, slot: Slot) -> bool {
        if let Some(next_active_epoch) = self.next_active_epoch {
            return (slot.epoch + 1 == next_active_epoch && self.within_handover_period(slot.slot))
                || (slot.epoch == next_active_epoch && !self.within_handover_period(slot.slot));
        }
        false
    }

    pub fn can_confirm(&self, slot: Slot) -> bool {
        if let Some(next_active_epoch) = self.next_active_epoch {
            return slot.epoch == next_active_epoch;
        }
        false
    }

    pub fn is_first_preconfirmation_slot(&self, slot: Slot) -> bool {
        if let Some(next_active_epoch) = self.next_active_epoch {
            return slot.epoch + 1 == next_active_epoch
                && slot.slot == self.slots_per_epoch - self.handover_slots;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SLOTS_PER_EPOCH: u64 = 10;

    #[test]
    fn if_no_active_epoch_is_set_then_can_not_preconfirm() {
        let handover_slots = u64::MAX;
        let model = ActiveOperatorModel::new(handover_slots, TEST_SLOTS_PER_EPOCH);

        let slot = Slot::new(0, 0);
        assert!(!model.can_preconfirm(slot));
    }

    #[test]
    fn if_more_than_handover_slots_behind_active_epoch_then_can_not_preconfirm() {
        let handover_slots = 3u64;
        let mut model = ActiveOperatorModel::new(handover_slots, TEST_SLOTS_PER_EPOCH);
        model.set_next_active_epoch(1);

        let slot = Slot::new(0, TEST_SLOTS_PER_EPOCH - handover_slots - 1);
        assert!(!model.can_preconfirm(slot));
    }

    #[test]
    fn if_exactly_handover_slots_behind_active_epoch_then_can_preconfirm() {
        let handover_slots = 3u64;
        let mut model = ActiveOperatorModel::new(handover_slots, TEST_SLOTS_PER_EPOCH);
        model.set_next_active_epoch(1);

        let slot = Slot::new(0, TEST_SLOTS_PER_EPOCH - handover_slots);
        assert!(model.can_preconfirm(slot));
    }

    #[test]
    fn if_in_active_epoch_can_preconfirm() {
        let handover_slots = 3u64;
        let mut model = ActiveOperatorModel::new(handover_slots, TEST_SLOTS_PER_EPOCH);
        model.set_next_active_epoch(1);

        let slot = Slot::new(1, 0);
        assert!(model.can_preconfirm(slot));
    }

    #[test]
    fn if_after_active_epoch_can_not_preconfirm() {
        let handover_slots = 3u64;
        let mut model = ActiveOperatorModel::new(handover_slots, TEST_SLOTS_PER_EPOCH);
        model.set_next_active_epoch(1);

        let slot = Slot::new(2, 0);
        assert!(!model.can_preconfirm(slot));
    }

    #[test]
    fn if_less_than_handover_slots_from_end_active_epoch_can_not_preconfirm() {
        let handover_slots = 3u64;
        let mut model = ActiveOperatorModel::new(handover_slots, TEST_SLOTS_PER_EPOCH);
        model.set_next_active_epoch(1);

        let slot = Slot::new(1, 7);
        assert!(!model.can_preconfirm(slot));
    }

    #[test]
    fn is_first_slot_in_preconfirmation_window() {
        let handover_slots = 3u64;
        let mut model = ActiveOperatorModel::new(handover_slots, TEST_SLOTS_PER_EPOCH);
        model.set_next_active_epoch(1);

        let slot = Slot::new(0, 7);
        assert!(model.is_first_preconfirmation_slot(slot));
    }

    #[test]
    fn is_not_first_slot_in_preconfirmation_window() {
        let handover_slots = 3u64;
        let mut model = ActiveOperatorModel::new(handover_slots, TEST_SLOTS_PER_EPOCH);
        model.set_next_active_epoch(1);

        let slot = Slot::new(0, 8);
        assert!(!model.is_first_preconfirmation_slot(slot));
    }

    #[test]
    fn if_no_active_epoch_is_set_then_is_not_first_slot_in_preconfirmation_window() {
        let handover_slots = u64::MAX;
        let model = ActiveOperatorModel::new(handover_slots, TEST_SLOTS_PER_EPOCH);

        let slot = Slot::new(0, 8);
        assert!(!model.is_first_preconfirmation_slot(slot));
    }

    #[test]
    fn if_no_active_epoch_is_set_then_can_not_confirm() {
        let handover_slots = u64::MAX;
        let model = ActiveOperatorModel::new(handover_slots, TEST_SLOTS_PER_EPOCH);

        let slot = Slot::new(0, 0);
        assert!(!model.can_confirm(slot));
    }

    #[test]
    fn if_in_active_epoch_can_confirm() {
        let handover_slots = 3u64;
        let mut model = ActiveOperatorModel::new(handover_slots, TEST_SLOTS_PER_EPOCH);
        model.set_next_active_epoch(1);

        let slot = Slot::new(1, 0);
        assert!(model.can_confirm(slot));
    }

    #[test]
    fn if_behind_active_epoch_can_not_confirm() {
        let handover_slots = 3u64;
        let mut model = ActiveOperatorModel::new(handover_slots, TEST_SLOTS_PER_EPOCH);
        model.set_next_active_epoch(1);

        let slot = Slot::new(0, 0);
        assert!(!model.can_confirm(slot));
    }

    #[test]
    fn if_after_active_epoch_can_not_confirm() {
        let handover_slots = 3u64;
        let mut model = ActiveOperatorModel::new(handover_slots, TEST_SLOTS_PER_EPOCH);
        model.set_next_active_epoch(1);

        let slot = Slot::new(2, 0);
        assert!(!model.can_confirm(slot));
    }

    #[test]
    fn within_handover_period() {
        let handover_slots = 3u64;
        let model = ActiveOperatorModel::new(handover_slots, TEST_SLOTS_PER_EPOCH);

        let slot = 0;
        assert!(!model.within_handover_period(slot));
        let slot = 6;
        assert!(!model.within_handover_period(slot));
    }

    #[test]
    fn not_within_handover_period() {
        let handover_slots = 3u64;
        let model = ActiveOperatorModel::new(handover_slots, TEST_SLOTS_PER_EPOCH);

        let slot = 7;
        assert!(model.within_handover_period(slot));
        let slot = 9;
        assert!(model.within_handover_period(slot));
    }
}
