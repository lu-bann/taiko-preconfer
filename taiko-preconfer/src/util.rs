use alloy_primitives::Address;
use preconfirmation::{taiko::contracts::TaikoWhitelistInstance, util::log_error};

#[derive(Debug, Clone, PartialEq)]
pub struct ActiveOperator {
    epoch: u64,
    operator: Address,
}

impl ActiveOperator {
    pub const fn new(epoch: u64, operator: Address) -> Self {
        Self { epoch, operator }
    }
}

impl Default for ActiveOperator {
    fn default() -> Self {
        Self {
            epoch: 0,
            operator: Address::ZERO,
        }
    }
}

#[derive(Debug)]
pub struct WhitelistMonitor {
    operators: Vec<ActiveOperator>,
    whitelist: TaikoWhitelistInstance,
}

impl WhitelistMonitor {
    pub fn new(whitelist: TaikoWhitelistInstance) -> Self {
        Self {
            operators: vec![],
            whitelist,
        }
    }

    pub fn is_current_and_next(&self, preconfer: Address) -> bool {
        self.operators.len() == 2
            && self
                .operators
                .iter()
                .all(|operator| operator.operator == preconfer)
    }

    pub fn is_active_in(&self, preconfer: Address, epoch: u64) -> bool {
        self.operators
            .contains(&ActiveOperator::new(epoch, preconfer))
    }

    pub fn change_epoch(&mut self, current_epoch: u64) {
        self.operators
            .retain(|operator| operator.epoch >= current_epoch);
    }

    pub async fn update_current_operator(&mut self, epoch: u64) {
        if let Some(operator) = log_error(
            self.whitelist.getOperatorForCurrentEpoch().call().await,
            "Failed to read current preconfer",
        ) {
            self.operators.retain(|operator| operator.epoch > epoch);

            if self.operators.is_empty() {
                self.operators.push(ActiveOperator::new(epoch, operator));
            } else {
                self.operators
                    .insert(0, ActiveOperator::new(epoch, operator));
            }
        }
    }

    pub async fn update_next_operator(&mut self, epoch: u64) {
        if let Some(operator) = log_error(
            self.whitelist.getOperatorForNextEpoch().call().await,
            "Failed to read preconfer for next epoch",
        ) {
            let next_epoch = epoch + 1;
            self.operators
                .retain(|operator| operator.epoch >= epoch && operator.epoch <= epoch + 1);
            if self.operators.last().cloned().unwrap_or_default().epoch < next_epoch {
                self.operators
                    .push(ActiveOperator::new(next_epoch, operator));
            } else if let Some(last) = self.operators.last_mut() {
                last.operator = operator;
            }
        }
    }
}
