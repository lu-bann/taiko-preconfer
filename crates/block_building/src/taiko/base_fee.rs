use alloy_contract::Error as ContractError;
use alloy_json_rpc::RpcError;
use alloy_primitives::{Address, U256};
use alloy_provider::{
    ProviderBuilder, RootProvider,
    fillers::{BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller},
};
use alloy_transport::TransportErrorKind;

use crate::taiko::contracts::TaikoAnchor;

pub type TaikoAnchorInstance = TaikoAnchor::TaikoAnchorInstance<
    FillProvider<
        JoinFill<
            alloy_provider::Identity,
            JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>,
        >,
        RootProvider,
    >,
>;

pub async fn get_taiko_anchor_instance(
    rpc_url: &str,
    contract_address: Address,
) -> Result<TaikoAnchorInstance, RpcError<TransportErrorKind>> {
    let provider = ProviderBuilder::new().connect(rpc_url).await?;
    Ok(TaikoAnchor::new(contract_address, provider))
}

pub async fn get_base_fee(
    anchor_instance: &TaikoAnchorInstance,
    base_fee_config: TaikoAnchor::BaseFeeConfig,
    parent_gas_used: u32,
    timestamp: u64,
) -> Result<U256, ContractError> {
    let basefee =
        anchor_instance.getBasefeeV2(parent_gas_used, timestamp, base_fee_config).call().await?;
    Ok(basefee.basefee_)
}
