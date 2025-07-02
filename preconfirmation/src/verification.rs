use alloy_eips::{
    BlockNumberOrTag,
    eip4844::{env_settings::EnvKzgSettings, kzg_to_versioned_hash},
};
use alloy_primitives::{Address, B256, Bytes, FixedBytes, Signature, SignatureError, keccak256};
use alloy_provider::Provider;
use alloy_rpc_types_eth::Block;
use alloy_sol_types::SolValue;
use c_kzg::Blob;
use thiserror::Error;
use tracing::{debug, info};

use crate::{
    blob::tx_bytes_to_blobs,
    compression::compress,
    taiko::{
        anchor::ValidAnchor,
        contracts::{
            BaseFeeConfig, Batch, BatchInfo, BatchMetadata, BlockParams, TaikoInboxInstance,
        },
        taiko_l1_client::TaikoL1ClientError,
    },
    util::{get_tx_envelopes_without_anchor_from_blocks, now_as_millis, pad_left},
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
    BlobEncode(#[from] crate::blob::BlobEncodeError),

    #[error("{0}")]
    RpcError(#[from] alloy_json_rpc::RpcError<alloy_transport::TransportErrorKind>),

    #[error("{0}")]
    Compression(#[from] libdeflater::CompressionError),

    #[error("{0}")]
    Contract(#[from] alloy_contract::Error),

    #[error("{0}")]
    Kzg(#[from] c_kzg::Error),
}

pub async fn get_latest_confirmed_batch(
    taiko_inbox: &TaikoInboxInstance,
) -> Result<Batch, TaikoInboxError> {
    let stats2 = taiko_inbox.getStats2().call().await?;
    let batch = taiko_inbox.getBatch(stats2.numBatches - 1).call().await?;
    Ok(batch)
}

pub async fn verify_last_batch(
    taiko_inbox: &TaikoInboxInstance,
    valid_anchor: ValidAnchor,
    preconfer_address: Address,
    unconfirmed_l2_blocks: Vec<Block>,
    base_fee_config: BaseFeeConfig,
    use_blobs: bool,
) -> Result<bool, TaikoL1ClientError> {
    let mut valid_anchor = valid_anchor;
    let stats2 = taiko_inbox.getStats2().call().await?;
    debug!("stats {stats2:?}");
    let batch = taiko_inbox.getBatch(stats2.numBatches - 1).call().await?;
    debug!("batch {batch:?}");
    valid_anchor
        .update_last_anchor_id(batch.anchorBlockId)
        .await?;
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
    let confirmed_batch_meta_hash = batch.metaHash;
    let computed_last_batch_meta_hash = compute_batch_meta_hash(
        preconfer_address,
        blocks,
        batch,
        anchor_header.hash_slow(),
        submit_header.timestamp,
        propose_batch_block_number,
        base_fee_config,
        use_blobs,
    )?;
    Ok(computed_last_batch_meta_hash == confirmed_batch_meta_hash)
}

#[cfg_attr(test, mockall::automock)]
pub trait ILastBatchVerifier {
    fn verify(
        &self,
        unconfirmed_l2_blocks: Vec<Block>,
    ) -> impl Future<Output = Result<bool, TaikoL1ClientError>>;
}

#[derive(Debug)]
pub struct LastBatchVerifier {
    taiko_inbox: TaikoInboxInstance,
    preconfer_address: Address,
    base_fee_config: BaseFeeConfig,
    use_blobs: bool,
    valid_anchor: ValidAnchor,
}

impl LastBatchVerifier {
    pub fn new(
        taiko_inbox: TaikoInboxInstance,
        preconfer_address: Address,
        base_fee_config: BaseFeeConfig,
        use_blobs: bool,
        valid_anchor: ValidAnchor,
    ) -> Self {
        Self {
            taiko_inbox,
            preconfer_address,
            base_fee_config,
            use_blobs,
            valid_anchor,
        }
    }
}

impl ILastBatchVerifier for LastBatchVerifier {
    async fn verify(&self, unconfirmed_l2_blocks: Vec<Block>) -> Result<bool, TaikoL1ClientError> {
        verify_last_batch(
            &self.taiko_inbox,
            self.valid_anchor.clone(),
            self.preconfer_address,
            unconfirmed_l2_blocks,
            self.base_fee_config.clone(),
            self.use_blobs,
        )
        .await
    }
}

#[allow(clippy::too_many_arguments)]
pub fn compute_batch_meta_hash(
    preconfer_address: Address,
    blocks: Vec<Block>,
    batch: Batch,
    anchor_hash: B256,
    proposed_at: u64,
    proposed_in: u64,
    base_fee_config: BaseFeeConfig,
    use_blobs: bool,
) -> Result<B256, TaikoL1ClientError> {
    info!("Compute batch meta hash, last batch:");
    info!("{:?}", batch);
    let mut last_timestamp = blocks
        .first()
        .expect("Batch must have at least one block")
        .header
        .timestamp;
    let block_params = blocks
        .iter()
        .map(|block| {
            info!(
                "block: {} {}",
                block.header.number,
                block.transactions.len()
            );
            let time_shift = block.header.timestamp - last_timestamp;
            last_timestamp = block.header.timestamp;
            BlockParams {
                numTransactions: block.transactions.len() as u16 - 1,
                timeShift: time_shift as u8,
                signalSlots: vec![],
            }
        })
        .collect();
    info!("last timestamp {last_timestamp}");

    info!("get envelopes {}", now_as_millis());
    let txs = get_tx_envelopes_without_anchor_from_blocks(blocks);
    debug!("txs: {txs:?}");
    let tx_bytes = Bytes::from(compress(txs)?);
    let tx_bytes_len = tx_bytes.len();

    info!("blb hashes {}", now_as_millis());
    let (tx_bytes_hash, blob_hashes) = if use_blobs {
        let blobs = tx_bytes_to_blobs(tx_bytes)?;
        (keccak256([]), get_blob_hashes(blobs)?)
    } else {
        (keccak256(&tx_bytes), vec![])
    };

    let mut abi_encoded = <(FixedBytes<32>, Vec<FixedBytes<32>>) as SolValue>::abi_encode(&(
        tx_bytes_hash,
        blob_hashes.clone(),
    ));
    let abi_encoded: Vec<_> = abi_encoded.drain(32..).collect();
    let txs_hash = keccak256(&abi_encoded);

    debug!("txs_hash {}", txs_hash);

    let info = BatchInfo {
        txsHash: txs_hash,
        blocks: block_params,
        blobHashes: blob_hashes,
        extraData: B256::from_slice(&pad_left::<32>(&[base_fee_config.sharingPctg])),
        coinbase: preconfer_address,
        proposedIn: proposed_in,
        blobCreatedIn: proposed_in,
        blobByteOffset: 0,
        blobByteSize: tx_bytes_len as u32,
        gasLimit: 240_000_000,
        lastBlockId: batch.lastBlockId,
        lastBlockTimestamp: batch.lastBlockTimestamp,
        anchorBlockId: batch.anchorBlockId,
        anchorBlockHash: anchor_hash,
        baseFeeConfig: base_fee_config,
    };
    info!("info: {info:?}");
    let meta = BatchMetadata {
        infoHash: keccak256(info.abi_encode()),
        proposer: preconfer_address,
        batchId: batch.batchId,
        proposedAt: proposed_at,
    };
    info!("meta: {meta:?}");
    info!("end {}", now_as_millis());
    Ok(keccak256(meta.abi_encode()))
}

fn get_blob_hashes(blobs: Vec<Blob>) -> Result<Vec<FixedBytes<32>>, TaikoInboxError> {
    let kzg_settings = EnvKzgSettings::Default.get();

    let blob_hashes: Result<_, _> = blobs
        .iter()
        .map(|blob| -> Result<FixedBytes<32>, c_kzg::Error> {
            let commitment = kzg_settings.blob_to_kzg_commitment(blob)?;
            let hash = kzg_to_versioned_hash(commitment.as_slice());
            info!("blob hash: {:?}", hash);
            Ok(hash)
        })
        .collect();
    Ok(blob_hashes?)
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
