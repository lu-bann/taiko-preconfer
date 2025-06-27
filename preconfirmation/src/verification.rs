use alloy_eips::BlockNumberOrTag;
use alloy_primitives::{Address, B256, FixedBytes, Signature, SignatureError, keccak256};
use alloy_provider::Provider;
use alloy_rpc_types_eth::Block;
use alloy_sol_types::SolValue;
use thiserror::Error;
use tracing::{debug, info};

use crate::{
    compression::compress,
    taiko::contracts::{TaikoInbox, TaikoInboxInstance},
    util::{get_tx_envelopes_from_blocks, pad_left},
};

pub fn verify_signature(
    signature: &Signature,
    message_hash: &FixedBytes<32>,
    signer_address: &Address,
) -> Result<bool, SignatureError> {
    match signature.recover_address_from_prehash(message_hash) {
        Ok(recovered_address) => Ok(&recovered_address == signer_address),
        Err(e) => Err(e),
    }
}

#[derive(Debug, Error)]
pub enum TaikoInboxError {
    #[error("{0}")]
    RpcError(#[from] alloy_json_rpc::RpcError<alloy_transport::TransportErrorKind>),

    #[error("{0}")]
    Compression(#[from] libdeflater::CompressionError),

    #[error("{0}")]
    Contract(#[from] alloy_contract::Error),

    #[error("{0}")]
    Http(#[from] crate::client::HttpError),
}

pub async fn get_latest_confirmed_batch(
    taiko_inbox: &TaikoInboxInstance,
) -> Result<TaikoInbox::Batch, TaikoInboxError> {
    let stats2 = taiko_inbox.getStats2().call().await?;
    let batch = taiko_inbox.getBatch(stats2.numBatches - 1).call().await?;
    Ok(batch)
}

pub async fn verify_last_batch(
    taiko_inbox: &TaikoInboxInstance,
    preconfer_address: Address,
    unconfirmed_l2_blocks: Vec<Block>,
) -> Result<bool, TaikoInboxError> {
    let stats2 = taiko_inbox.getStats2().call().await?;
    debug!("stats {stats2:?}");
    let batch = taiko_inbox.getBatch(stats2.numBatches - 1).call().await?;
    debug!("batch {batch:?}");
    let blocks: Vec<_> = unconfirmed_l2_blocks
        .into_iter()
        .filter(|block| block.header.number <= batch.lastBlockId)
        .collect();
    if blocks.is_empty() {
        return Ok(true);
    }

    debug!("#blocks {}", blocks.len());
    let anchor_header = taiko_inbox
        .provider()
        .get_block_by_number(BlockNumberOrTag::Number(batch.anchorBlockId))
        .await?
        .expect("TODO: can be re-orged away by L1 reorg")
        .header;
    let propose_batch_block_number = stats2.lastProposedIn.as_limbs()[0];
    let submit_header = taiko_inbox
        .provider()
        .get_block_by_number(BlockNumberOrTag::Number(propose_batch_block_number))
        .await?
        .expect("TODO: can be re-orged away by L1 reorg")
        .header;
    let base_fee_config = taiko_inbox.pacayaConfig().call().await?.baseFeeConfig;
    let confirmed_batch_meta_hash = batch.metaHash;
    let computed_last_batch_meta_hash = compute_batch_meta_hash(
        preconfer_address,
        blocks,
        batch,
        anchor_header.hash_slow(),
        submit_header.timestamp,
        propose_batch_block_number,
        base_fee_config,
    )?;
    Ok(computed_last_batch_meta_hash == confirmed_batch_meta_hash)
}

#[cfg_attr(test, mockall::automock)]
pub trait ILastBatchVerifier {
    fn verify(
        &self,
        unconfirmed_l2_blocks: Vec<Block>,
    ) -> impl Future<Output = Result<bool, TaikoInboxError>>;
}

#[derive(Debug)]
pub struct LastBatchVerifier {
    taiko_inbox: TaikoInboxInstance,
    preconfer_address: Address,
}

impl LastBatchVerifier {
    pub fn new(taiko_inbox: TaikoInboxInstance, preconfer_address: Address) -> Self {
        Self {
            taiko_inbox,
            preconfer_address,
        }
    }
}

impl ILastBatchVerifier for LastBatchVerifier {
    async fn verify(&self, unconfirmed_l2_blocks: Vec<Block>) -> Result<bool, TaikoInboxError> {
        verify_last_batch(
            &self.taiko_inbox,
            self.preconfer_address,
            unconfirmed_l2_blocks,
        )
        .await
    }
}

pub fn compute_txs_hash(blocks: Vec<Block>) -> Result<B256, TaikoInboxError> {
    let txs = get_tx_envelopes_from_blocks(blocks);
    debug!("txs: {txs:?}");
    let compressed_txs = compress(txs)?;

    let mut abi_encoded = <(FixedBytes<32>, Vec<FixedBytes<32>>) as SolValue>::abi_encode(&(
        keccak256(&compressed_txs),
        vec![],
    ));
    let abi_encoded: Vec<_> = abi_encoded.drain(32..).collect();
    Ok(keccak256(&abi_encoded))
}

pub fn compute_batch_meta_hash(
    preconfer_address: Address,
    blocks: Vec<Block>,
    batch: TaikoInbox::Batch,
    anchor_hash: B256,
    proposed_at: u64,
    proposed_in: u64,
    base_fee_config: TaikoInbox::BaseFeeConfig,
) -> Result<B256, TaikoInboxError> {
    let mut last_timestamp = blocks
        .first()
        .expect("Batch must have at least one block")
        .header
        .timestamp;
    let block_params = blocks
        .iter()
        .map(|block| {
            let time_shift = block.header.timestamp - last_timestamp;
            last_timestamp = block.header.timestamp;
            TaikoInbox::BlockParams {
                numTransactions: block.transactions.len() as u16,
                timeShift: time_shift as u8,
                signalSlots: vec![],
            }
        })
        .collect();
    info!("last timestamp {last_timestamp}");

    let txs = get_tx_envelopes_from_blocks(blocks);
    debug!("txs: {txs:?}");
    let compressed_txs = compress(txs)?;

    let mut abi_encoded = <(FixedBytes<32>, Vec<FixedBytes<32>>) as SolValue>::abi_encode(&(
        keccak256(&compressed_txs),
        vec![],
    ));
    let abi_encoded: Vec<_> = abi_encoded.drain(32..).collect();
    let txs_hash = keccak256(&abi_encoded);
    debug!("txs_hash {}", txs_hash);

    let txs_bytes = compressed_txs.len();
    let info = TaikoInbox::BatchInfo {
        txsHash: txs_hash,
        blocks: block_params,
        blobHashes: vec![],
        extraData: B256::from_slice(&pad_left::<32>(&[base_fee_config.sharingPctg])),
        coinbase: preconfer_address,
        proposedIn: proposed_in,
        blobCreatedIn: 0,
        blobByteOffset: 0,
        blobByteSize: txs_bytes as u32,
        gasLimit: 240_000_000,
        lastBlockId: batch.lastBlockId,
        lastBlockTimestamp: batch.lastBlockTimestamp,
        anchorBlockId: batch.anchorBlockId,
        anchorBlockHash: anchor_hash,
        baseFeeConfig: base_fee_config,
    };
    info!("info: {info:?}");
    let meta = TaikoInbox::BatchMetadata {
        infoHash: keccak256(info.abi_encode()),
        proposer: preconfer_address,
        batchId: batch.batchId,
        proposedAt: proposed_at,
    };
    debug!("meta: {meta:?}");
    Ok(keccak256(meta.abi_encode()))
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{U256, eip191_hash_message};
    use alloy_signer::SignerSync;
    use alloy_signer_local::PrivateKeySigner;

    use super::*;

    #[test]
    fn verification_passes_for_valid_signature() {
        let signer = PrivateKeySigner::random();
        let message = b"some msg";
        let signature = signer.sign_message_sync(message).unwrap();

        let message_hash = eip191_hash_message(message);
        let is_valid = verify_signature(&signature, &message_hash, &signer.address()).unwrap();
        assert!(is_valid);
    }

    #[test]
    fn verification_fails_for_wrong_signer() {
        let signer = PrivateKeySigner::random();
        let not_signer = PrivateKeySigner::random();
        let message = b"some msg";
        let signature = signer.sign_message_sync(message).unwrap();

        let message_hash = eip191_hash_message(message);
        let is_valid = verify_signature(&signature, &message_hash, &not_signer.address()).unwrap();
        assert!(!is_valid);
    }

    #[test]
    fn verification_throws_for_invalid_signature() {
        let not_signer = PrivateKeySigner::random();
        let message = b"some msg";

        let message_hash = eip191_hash_message(message);
        let signature = Signature::new(U256::ZERO, U256::ZERO, false);
        assert!(verify_signature(&signature, &message_hash, &not_signer.address()).is_err());
    }

    #[test]
    fn verification_fails_for_wrong_message_hash() {
        let signer = PrivateKeySigner::random();
        let message = b"some msg";
        let signature = signer.sign_message_sync(message).unwrap();

        let wrong_message = b"other msg";
        let wrong_message_hash = eip191_hash_message(wrong_message);
        let is_valid =
            verify_signature(&signature, &wrong_message_hash, &signer.address()).unwrap();
        assert!(!is_valid);
    }
}
