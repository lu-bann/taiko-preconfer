use std::time::Duration;

use crate::slot::Slot;

#[derive(Debug, Clone, Copy)]
pub struct SlotModel {
    pub genesis_timestamp: u64,
    pub slot_duration: Duration,
    pub epoch_duration: Duration,
}

impl SlotModel {
    pub const fn new(
        genesis_timestamp: u64,
        slot_duration: Duration,
        epoch_duration: Duration,
    ) -> Self {
        Self {
            genesis_timestamp,
            slot_duration,
            epoch_duration,
        }
    }

    pub fn get_slot(&self, timestamp: u64) -> Slot {
        let diff = timestamp - self.genesis_timestamp;
        Slot {
            epoch: diff / self.epoch_duration.as_secs(),
            slot: (diff % self.epoch_duration.as_secs()) / self.slot_duration.as_secs(),
        }
    }

    pub fn get_timestamp(&self, slot: u64) -> u64 {
        self.genesis_timestamp + self.slot_duration.as_secs() * slot
    }

    pub fn get_slot_number(&self, slot: Slot) -> u64 {
        slot.epoch * self.slots_per_epoch() + slot.slot
    }

    pub fn slots_per_epoch(&self) -> u64 {
        self.epoch_duration.as_secs() / self.slot_duration.as_secs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_GENESIS_TIMESTAMP: u64 = 12345;
    const TEST_SLOT_DURATION: Duration = Duration::from_secs(10);
    const TEST_EPOCH_DURATION: Duration = Duration::from_secs(50);

    #[test]
    fn slot_model_get_timestamp_for_slot_0() {
        let model = SlotModel::new(
            TEST_GENESIS_TIMESTAMP,
            TEST_SLOT_DURATION,
            TEST_EPOCH_DURATION,
        );

        let ts = model.get_timestamp(0);
        assert_eq!(ts, TEST_GENESIS_TIMESTAMP);
    }

    #[test]
    fn slot_model_get_timestamp_for_slot_3() {
        let model = SlotModel::new(
            TEST_GENESIS_TIMESTAMP,
            TEST_SLOT_DURATION,
            TEST_EPOCH_DURATION,
        );

        let ts = model.get_timestamp(3);
        assert_eq!(
            ts,
            TEST_GENESIS_TIMESTAMP + TEST_SLOT_DURATION.as_secs() * 3
        );
    }

    #[test]
    fn slots_per_epoch() {
        let model = SlotModel::new(
            TEST_GENESIS_TIMESTAMP,
            TEST_SLOT_DURATION,
            TEST_EPOCH_DURATION,
        );

        let slots_per_epoch = model.slots_per_epoch();
        assert_eq!(slots_per_epoch, 5);
    }

    #[test]
    fn get_total_slot_number_from_slot() {
        let model = SlotModel::new(
            TEST_GENESIS_TIMESTAMP,
            TEST_SLOT_DURATION,
            TEST_EPOCH_DURATION,
        );

        let epoch = 10;
        let slot = 3;
        let slot = Slot::new(epoch, slot);
        let slot_number = model.get_slot_number(slot);
        assert_eq!(slot_number, 53);
    }

    #[test]
    fn slot_model_epoch_0_slot_0() {
        let model = SlotModel::new(
            TEST_GENESIS_TIMESTAMP,
            TEST_SLOT_DURATION,
            TEST_EPOCH_DURATION,
        );

        let slot = model.get_slot(TEST_GENESIS_TIMESTAMP);
        assert_eq!(slot, Slot::new(0, 0));
    }

    #[test]
    fn slot_model_epoch_0_slot_0_inexact() {
        let model = SlotModel::new(
            TEST_GENESIS_TIMESTAMP,
            TEST_SLOT_DURATION,
            TEST_EPOCH_DURATION,
        );

        let slot = model.get_slot(TEST_GENESIS_TIMESTAMP + TEST_SLOT_DURATION.as_secs() / 2);
        assert_eq!(slot, Slot::new(0, 0));
    }

    #[test]
    fn slot_model_epoch_1_slot_0() {
        let model = SlotModel::new(
            TEST_GENESIS_TIMESTAMP,
            TEST_SLOT_DURATION,
            TEST_EPOCH_DURATION,
        );

        let slot = model.get_slot(TEST_GENESIS_TIMESTAMP + TEST_EPOCH_DURATION.as_secs());
        assert_eq!(slot, Slot::new(1, 0));
    }

    #[test]
    fn slot_model_epoch_1_slot_3() {
        let model = SlotModel::new(
            TEST_GENESIS_TIMESTAMP,
            TEST_SLOT_DURATION,
            TEST_EPOCH_DURATION,
        );

        let slot = model.get_slot(
            TEST_GENESIS_TIMESTAMP
                + TEST_EPOCH_DURATION.as_secs()
                + 3 * TEST_SLOT_DURATION.as_secs(),
        );
        assert_eq!(slot, Slot::new(1, 3));
    }
}
