use std::time::Duration;

use alloy_eips::merge::{EPOCH_DURATION, SLOT_DURATION};

use crate::slot::Slot;

pub const HOLESKY_GENESIS_TIMESTAMP: u64 = 1_695_902_100;
pub const TAIKO_HOLESKY_GENESIS_TIMESTAMP: u64 = 1_711_697_940;

#[derive(Debug)]
pub struct SlotModel {
    genesis_timestamp: u64,
    slot_duration: Duration,
    epoch_duration: Duration,
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

    pub const fn holesky() -> Self {
        Self::new(HOLESKY_GENESIS_TIMESTAMP, SLOT_DURATION, EPOCH_DURATION)
    }

    pub const fn taiko_holesky(slot_duration: Duration) -> Self {
        Self::new(
            TAIKO_HOLESKY_GENESIS_TIMESTAMP,
            slot_duration,
            EPOCH_DURATION,
        )
    }

    pub fn get_slot(&self, timestamp: u64) -> Slot {
        let diff = timestamp - self.genesis_timestamp;
        Slot {
            epoch: diff / self.epoch_duration.as_secs(),
            slot: (diff % self.epoch_duration.as_secs()) / self.slot_duration.as_secs(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_GENESIS_TIMESTAMP: u64 = 12345;
    const TEST_SLOT_DURATION: Duration = Duration::from_secs(10);
    const TEST_EPOCH_DURATION: Duration = Duration::from_secs(50);

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
