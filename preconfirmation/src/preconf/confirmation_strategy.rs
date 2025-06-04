use alloy_consensus::{SignableTransaction, TxEip1559, TxEnvelope};
use alloy_primitives::{Address, B256, Bytes, ChainId};
use alloy_provider::network::TransactionBuilder as _;
use alloy_rpc_types::TransactionRequest;
use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;
use thiserror::Error;
use tokio::join;
use tracing::{debug, error, info};

use crate::{
    compression::compress,
    taiko::{
        contracts::taiko_wrapper::BlockParams,
        hekla::addresses::{get_golden_touch_signing_key, get_taiko_inbox_address},
        propose_batch::create_propose_batch_params,
        taiko_l1_client::ITaikoL1Client,
    },
};

use super::SimpleBlock;

#[derive(Debug, Error)]
pub enum ConfirmationError {
    #[error("{0}")]
    Compression(#[from] libdeflater::CompressionError),

    #[error("{0}")]
    Http(#[from] crate::client::HttpError),

    #[error("{0}")]
    Signer(#[from] alloy_signer::Error),

    #[error("{0}")]
    TaikoL1Client(#[from] crate::taiko::taiko_l1_client::TaikoL1ClientError),
}

pub type ConfirmationResult<T> = Result<T, ConfirmationError>;

#[derive(Debug)]
pub struct InstantConfirmationStrategy<Client: ITaikoL1Client> {
    client: Client,
    address: Address,
    chain_id: ChainId,
}

impl<Client: ITaikoL1Client> InstantConfirmationStrategy<Client> {
    pub const fn new(client: Client, address: Address, chain_id: ChainId) -> Self {
        Self {
            client,
            address,
            chain_id,
        }
    }

    pub async fn confirm(&self, block: SimpleBlock) -> ConfirmationResult<()> {
        debug!("Compression");

        let number_of_blobs = 0u8;
        let parent_meta_hash = B256::ZERO;
        let tx_bytes = Bytes::from(compress(block.txs.clone())?);
        let block_params = vec![BlockParams {
            numTransactions: block.txs.len() as u16,
            timeShift: 0,
            signalSlots: vec![],
        }];
        info!("Create propose batch params");
        let propose_batch_params = create_propose_batch_params(
            self.address,
            tx_bytes,
            block_params,
            parent_meta_hash,
            block.anchor_block_id,
            block.last_block_timestamp,
            self.address,
            number_of_blobs,
        );

        debug!("Create tx");
        let signer = PrivateKeySigner::from_signing_key(get_golden_touch_signing_key());
        let taiko_inbox_address = get_taiko_inbox_address();
        let tx = TransactionRequest::default()
            .with_to(taiko_inbox_address)
            .with_input(propose_batch_params.clone())
            .with_from(signer.address());
        let signer_str = signer.address().to_string();
        let (nonce, gas_limit, fee_estimate) = join!(
            self.client.get_nonce(&signer_str),
            self.client.estimate_gas(tx),
            self.client.estimate_eip1559_fees(),
        );
        let fee_estimate = fee_estimate?;

        debug!(
            "sign tx {} {:?} {:?}",
            taiko_inbox_address, nonce, gas_limit
        );
        if gas_limit.is_err() {
            error!("Failed to estimate gas for block confirmation.");
        }
        let signed_tx = get_signed_eip1559_tx(
            self.chain_id,
            taiko_inbox_address,
            propose_batch_params,
            nonce?,
            gas_limit?,
            fee_estimate.max_fee_per_gas,
            fee_estimate.max_priority_fee_per_gas,
            &signer,
        )?;

        info!("signed propose batch tx {signed_tx:?}");
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn get_signed_eip1559_tx(
    chain_id: ChainId,
    to: Address,
    input: Bytes,
    nonce: u64,
    gas_limit: u64,
    max_fee_per_gas: u128,
    max_priority_fee_per_gas: u128,
    signer: &PrivateKeySigner,
) -> ConfirmationResult<TxEnvelope> {
    let tx = TxEip1559 {
        chain_id,
        nonce,
        gas_limit,
        max_fee_per_gas,
        max_priority_fee_per_gas,
        to: to.into(),
        input,
        value: Default::default(),
        access_list: Default::default(),
    };

    let sig = signer.sign_hash_sync(&tx.signature_hash())?;
    let signed = tx.into_signed(sig);

    Ok(signed.into())
}
