#[derive(Debug, PartialEq)]
pub struct Slot {
    pub epoch: u64,
    pub slot: u64,
}

impl Slot {
    pub const fn new(epoch: u64, slot: u64) -> Self {
        Self { epoch, slot }
    }
}

#[derive(Debug, PartialEq)]
pub struct SubSlot {
    pub slot: Slot,
    pub sub_slot: u64,
}

impl SubSlot {
    pub const fn new(slot: Slot, sub_slot: u64) -> Self {
        Self { slot, sub_slot }
    }
}
