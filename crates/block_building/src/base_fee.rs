use alloy_eips::BlockId;
use alloy_primitives::{Address, U256};
use alloy_provider::{Provider, ProviderBuilder};

use crate::{BASE_FEE_CONFIG_HEKLA, L2_BLOCK_TIME_MS, taiko::contracts::TaikoAnchor};

/// Calculates the base fee for next block using the TaikoAnchor contract.
pub async fn calculate_next_block_base_fee(
    rpc_url: &str,
    contract_address: Address,
) -> eyre::Result<U256> {
    let provider = ProviderBuilder::new().connect(rpc_url).await?;
    let parent_block = provider.get_block(BlockId::latest()).await?.unwrap();
    let parent_gas_used = parent_block.header.gas_used as u32;
    let block_timestamp = parent_block.header.timestamp + (L2_BLOCK_TIME_MS / 1000);

    let basefee = TaikoAnchor::TaikoAnchorInstance::new(contract_address, provider)
        .getBasefeeV2(parent_gas_used, block_timestamp, BASE_FEE_CONFIG_HEKLA)
        .call()
        .await?;
    Ok(basefee.basefee_)
}
