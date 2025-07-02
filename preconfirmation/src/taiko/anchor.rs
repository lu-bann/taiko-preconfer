use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use alloy_consensus::{TxEip1559, TypedTransaction};
use alloy_primitives::{Address, ChainId, FixedBytes, TxKind, U256};
use alloy_sol_types::SolCall;
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::{client::reqwest::get_header_by_id, taiko::contracts::TaikoAnchor};

const ANCHOR_GAS_LIMIT: u64 = 1_000_000;

pub fn create_anchor_transaction(
    chain_id: ChainId,
    nonce: u64,
    max_fee_per_gas: u128,
    max_priority_fee_per_gas: u128,
    taiko_anchor_address: Address,
    anchor_tx: TaikoAnchor::anchorV3Call,
) -> TypedTransaction {
    TypedTransaction::from(TxEip1559 {
        chain_id,
        nonce,
        gas_limit: ANCHOR_GAS_LIMIT,
        max_fee_per_gas,
        max_priority_fee_per_gas,
        to: TxKind::Call(taiko_anchor_address),
        value: U256::default(),
        access_list: vec![].into(),
        input: anchor_tx.abi_encode().into(),
    })
}

pub fn compute_valid_anchor_id(
    block_number: u64,
    max_offset: u64,
    desired_offset: u64,
    last_anchor_id: u64,
) -> u64 {
    let min_anchor_id = std::cmp::max(
        std::cmp::max(block_number, max_offset) - max_offset,
        last_anchor_id,
    );
    let desired_anchor_id = block_number - desired_offset;
    std::cmp::max(desired_anchor_id, min_anchor_id)
}

#[derive(Debug, Clone)]
pub struct ValidAnchor {
    max_offset: u64,
    desired_offset: u64,
    block_number: Arc<AtomicU64>,
    current_anchor: Arc<RwLock<(u64, FixedBytes<32>)>>,
    last_anchor_id: Arc<AtomicU64>,
    anchor_id_update_tol: u64,
    url: String,
}

impl ValidAnchor {
    pub fn new(
        max_offset: u64,
        desired_offset: u64,
        anchor_id_update_tol: u64,
        url: String,
    ) -> Self {
        Self {
            max_offset,
            desired_offset,
            block_number: Arc::new(0.into()),
            current_anchor: Arc::new((0, FixedBytes::default()).into()),
            last_anchor_id: Arc::new(0.into()),
            anchor_id_update_tol,
            url,
        }
    }

    pub async fn update(&mut self) -> Result<(), reqwest::Error> {
        let block_number = self.block_number.load(Ordering::Relaxed);
        let last_anchor_id = self.last_anchor_id.load(Ordering::Relaxed);
        debug!(
            "compute anchor id block={}, max_offset={}, desired_offset={}, last_anchor={}",
            block_number, self.max_offset, self.desired_offset, last_anchor_id
        );
        let new_anchor_id = compute_valid_anchor_id(
            block_number,
            self.max_offset,
            self.desired_offset,
            last_anchor_id,
        );
        let current_anchor_id = self.current_anchor.read().await.0;
        if new_anchor_id > current_anchor_id + self.anchor_id_update_tol {
            info!("Request header {} for {}", new_anchor_id, self.url);
            let anchor_header = get_header_by_id(self.url.clone(), new_anchor_id).await?;
            *self.current_anchor.write().await = (new_anchor_id, anchor_header.state_root);
            info!("anchor id update to {}", new_anchor_id);
        }
        Ok(())
    }

    pub async fn is_valid_after(&self, offset: u64) -> bool {
        self.current_anchor.read().await.0
            == compute_valid_anchor_id(
                self.block_number.load(Ordering::Relaxed) + offset,
                self.max_offset,
                self.desired_offset,
                self.last_anchor_id.load(Ordering::Relaxed),
            )
    }

    pub async fn is_valid(&self) -> bool {
        self.is_valid_after(0).await
    }

    pub async fn update_block_number(&mut self, block_number: u64) -> Result<(), reqwest::Error> {
        if block_number > self.block_number.load(Ordering::Relaxed) {
            self.block_number.store(block_number, Ordering::Relaxed);
            return self.update().await;
        }
        Ok(())
    }

    pub async fn update_last_anchor_id(
        &mut self,
        last_anchor_id: u64,
    ) -> Result<(), reqwest::Error> {
        self.last_anchor_id.store(last_anchor_id, Ordering::Relaxed);
        self.update().await
    }

    pub async fn id_and_state_root(&self) -> (u64, FixedBytes<32>) {
        *self.current_anchor.read().await
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use alloy_consensus::Transaction;
    use alloy_primitives::{Bytes, FixedBytes, address};

    use crate::{taiko::contracts::BaseFeeConfig, util::hex_decode};

    pub fn get_basefee_config_v2() -> BaseFeeConfig {
        BaseFeeConfig {
            adjustmentQuotient: 8,
            sharingPctg: 50,
            gasIssuancePerSecond: 5_000_000,
            minGasExcess: 1_344_899_430,
            maxGasIssuancePerBlock: 600_000_000,
        }
    }

    #[test]
    fn test_create_anchor_transaction() {
        let taiko_anchor_address = address!("0x1670090000000000000000000000000000010001");
        let my_anchor_call = TaikoAnchor::anchorV3Call {
            _anchorBlockId: 3794466,
            _anchorStateRoot: FixedBytes::from_slice(
                &hex_decode("0xf9c88fc529e27024f8b37ae1633931cc1bb0f772f13f912ae77d6771603b16cb")
                    .unwrap(),
            ),
            _parentGasUsed: 181596,
            _baseFeeConfig: get_basefee_config_v2(),
            _signalSlots: vec![],
        };

        let chain_id: ChainId = 167009;
        let nonce = 135343764;
        let max_fee_per_gas: u128 = 10_000_000u128;
        let max_priority_fee_per_gas = 0u128;
        let anchor_transaction = create_anchor_transaction(
            chain_id,
            nonce,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            taiko_anchor_address,
            my_anchor_call,
        );

        let expected_hex_str = "0x48080a45000000000000000000000000000000000000000000000000000000000039e622f9c88fc529e27024f8b37ae1633931cc1bb0f772f13f912ae77d6771603b16cb000000000000000000000000000000000000000000000000000000000002c55c0000000000000000000000000000000000000000000000000000000000000008000000000000000000000000000000000000000000000000000000000000003200000000000000000000000000000000000000000000000000000000004c4b4000000000000000000000000000000000000000000000000000000000502989660000000000000000000000000000000000000000000000000000000023c3460000000000000000000000000000000000000000000000000000000000000001200000000000000000000000000000000000000000000000000000000000000000";
        let expected_input = Bytes::copy_from_slice(&hex_decode(expected_hex_str).unwrap());
        assert_eq!(*anchor_transaction.input(), expected_input);
        assert_eq!(
            anchor_transaction.eip1559().unwrap().to,
            TxKind::Call(taiko_anchor_address)
        )
    }

    #[test]
    fn compute_valid_anchor_when_desired_anchor_id_is_within_bounds() {
        let block_number = 5;
        let max_offset = 12;
        let desired_offset = 2;
        let last_anchor_id = 3;
        let anchor_block_id =
            compute_valid_anchor_id(block_number, max_offset, desired_offset, last_anchor_id);
        assert_eq!(anchor_block_id, 3);
    }

    #[test]
    fn compute_valid_anchor_when_desired_anchor_id_behind_last_anchor() {
        let block_number = 5;
        let max_offset = 12;
        let desired_offset = 2;
        let last_anchor_id = 4;
        let anchor_block_id =
            compute_valid_anchor_id(block_number, max_offset, desired_offset, last_anchor_id);
        assert_eq!(anchor_block_id, 4);
    }

    #[test]
    fn compute_valid_anchor_when_desired_anchor_id_more_than_max_offset_behind_current_block() {
        let block_number = 5;
        let max_offset = 1;
        let desired_offset = 2;
        let last_anchor_id = 3;
        let anchor_block_id =
            compute_valid_anchor_id(block_number, max_offset, desired_offset, last_anchor_id);
        assert_eq!(anchor_block_id, 4);
    }
}
