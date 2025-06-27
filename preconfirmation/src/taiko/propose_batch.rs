use alloy_primitives::{Address, B256, Bytes};
use alloy_sol_types::SolType;
use tracing::info;

use super::contracts::taiko_wrapper::ProposeBatchParams;
use crate::taiko::contracts::taiko_wrapper::{BatchParams, BlobParams, BlockParams};

#[allow(clippy::too_many_arguments)]
pub fn create_propose_batch_params(
    proposer: Address,
    tx_bytes: usize,
    blocks: Vec<BlockParams>,
    parent_meta_hash: B256,
    anchor_block_id: u64,
    last_block_timestamp: u64,
    coinbase: Address,
    number_of_blobs: u8,
) -> Bytes {
    let batch_params = BatchParams {
        proposer,
        coinbase,
        parentMetaHash: parent_meta_hash,
        anchorBlockId: anchor_block_id,
        lastBlockTimestamp: last_block_timestamp,
        revertIfNotFirstProposal: false,
        blobParams: BlobParams {
            blobHashes: vec![],
            firstBlobIndex: 0,
            numBlobs: number_of_blobs,
            byteOffset: 0,
            byteSize: tx_bytes as u32,
            createdIn: 0,
        },
        blocks,
    };
    info!("batch params: {:?}", batch_params);

    let propose_batch_wrapper = ProposeBatchParams {
        bytesX: Bytes::new(),
        bytesY: Bytes::from(BatchParams::abi_encode(&batch_params)),
    };

    Bytes::from(ProposeBatchParams::abi_encode_sequence(
        &propose_batch_wrapper,
    ))
}
