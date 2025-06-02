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
