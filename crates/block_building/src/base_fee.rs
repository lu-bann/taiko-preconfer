use alloy_eips::BlockId;
use alloy_primitives::{Address, U256};
use alloy_provider::{Provider, ProviderBuilder};
use alloy_sol_types::sol;

use crate::L2_BLOCK_TIME_MS;

sol!(
    #[sol(rpc)]
    contract TaikoAnkor {
        #[derive(Debug)]
        struct BaseFeeConfig {
            uint8 adjustmentQuotient;
            uint8 sharingPctg;
            uint32 gasIssuancePerSecond;
            uint64 minGasExcess;
            uint32 maxGasIssuancePerBlock;
        }

        #[derive(Debug)]
        function getBasefeeV2(
            uint32 _parentGasUsed,
            uint64 _blockTimestamp,
            BaseFeeConfig calldata _baseFeeConfig
        )
            public
            view
            returns (uint256 basefee_, uint64 newGasTarget_, uint64 newGasExcess_);
    }
);

/// Calculates the base fee for next block using the TaikoAnkor contract.
pub async fn calculate_next_block_base_fee(
    rpc_url: &str,
    contract_address: Address,
) -> eyre::Result<U256> {
    let provider = ProviderBuilder::new().connect(rpc_url).await?;
    let parent_block = provider.get_block(BlockId::latest()).await?.unwrap();
    let parent_gas_used = parent_block.header.gas_used as u32;
    let block_timestamp = parent_block.header.timestamp + (L2_BLOCK_TIME_MS / 1000);

    // Values taken from https://github.com/taikoxyz/taiko-mono/blob/main/packages/protocol/contracts/layer1/hekla/HeklaInbox.sol#L94
    let base_fee_config = TaikoAnkor::BaseFeeConfig {
        adjustmentQuotient: 8,
        sharingPctg: 50,
        gasIssuancePerSecond: 5_000_000,
        minGasExcess: 1_344_899_430,
        maxGasIssuancePerBlock: 600_000_000,
    };
    let basefee = TaikoAnkor::TaikoAnkorInstance::new(contract_address, provider)
        .getBasefeeV2(parent_gas_used, block_timestamp, base_fee_config)
        .call()
        .await?;
    Ok(basefee.basefee_)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[tokio::test]
    async fn test_calculate_next_block_base_fee_hekla() {
        let rpc_url = "https://rpc.hekla.taiko.xyz";
        let contract_address =
            Address::from_str("0x1670090000000000000000000000000000010001").unwrap();

        let basefee = calculate_next_block_base_fee(rpc_url, contract_address).await.unwrap();

        println!("Base fee: {}", basefee);
    }
}
