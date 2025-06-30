use std::{num::TryFromIntError, sync::Arc};

use alloy_consensus::Transaction;
use alloy_primitives::{Address, B256, Bytes};
use alloy_provider::network::TransactionBuilder as _;
use alloy_rpc_types::TransactionRequest;
use alloy_rpc_types_eth::Block;
use alloy_signer_local::LocalSigner;
use alloy_sol_types::SolType;
use k256::ecdsa::SigningKey;
use thiserror::Error;
use tokio::{join, sync::RwLock};
use tracing::{debug, error, info};

use crate::{
    compression::compress,
    taiko::{
        anchor::ValidAnchor,
        contracts::{
            BatchParams, BlobParams, BlockParams, ProposeBatchParams, TaikoInbox,
            TaikoInboxInstance,
        },
        taiko_l1_client::ITaikoL1Client,
    },
    util::{
        ValidTimestamp, get_anchor_block_id_from_bytes, get_tx_envelopes_without_anchor_from_block,
    },
    verification::get_latest_confirmed_batch,
};

#[derive(Debug, Error)]
pub enum ConfirmationError {
    #[error("{0}")]
    Compression(#[from] libdeflater::CompressionError),

    #[error("{0}")]
    Http(#[from] crate::client::HttpError),

    #[error("{0}")]
    Signer(#[from] alloy_signer::Error),

    #[error("{0}")]
    SolTypes(#[from] alloy_sol_types::Error),

    #[error("{0}")]
    TryU8FromU64(#[from] TryFromIntError),

    #[error("{0}")]
    TaikoL1Client(#[from] crate::taiko::taiko_l1_client::TaikoL1ClientError),

    #[error("Empty block (id={0})")]
    EmptyBlock(u64),
}

pub type ConfirmationResult<T> = Result<T, ConfirmationError>;

#[derive(Debug, Clone)]
pub struct ConfirmationSender<Client: ITaikoL1Client> {
    client: Client,
    taiko_inbox: Address,
    signer: LocalSigner<SigningKey>,
}

impl<Client: ITaikoL1Client> ConfirmationSender<Client> {
    pub const fn new(
        client: Client,
        taiko_inbox: Address,
        signer: LocalSigner<SigningKey>,
    ) -> Self {
        Self {
            client,
            taiko_inbox,
            signer,
        }
    }

    pub fn address(&self) -> Address {
        self.signer.address()
    }

    pub async fn send(&self, tx: TransactionRequest) -> ConfirmationResult<()> {
        debug!("Create tx");
        let tx = tx
            .with_from(self.signer.address())
            .with_to(self.taiko_inbox);
        let (nonce, gas_limit, fee_estimate) = join!(
            self.client.get_nonce(self.signer.address()),
            self.client.estimate_gas(tx.clone()),
            self.client.estimate_eip1559_fees(),
        );

        let fee_estimate = fee_estimate?;

        info!(
            "sign tx {} {:?} {:?} {:?}",
            self.taiko_inbox, nonce, gas_limit, fee_estimate
        );
        if gas_limit.is_err() {
            error!("Failed to estimate gas for block confirmation.");
            error!("{}", gas_limit.unwrap_err());
            return Ok(());
        }
        let tx = tx
            .with_gas_limit(gas_limit?)
            .with_max_fee_per_gas(fee_estimate.max_fee_per_gas * 2)
            .with_max_priority_fee_per_gas(fee_estimate.max_priority_fee_per_gas * 2)
            .nonce(nonce?);

        info!("propose batch tx {tx:?}");
        self.client.send(tx).await?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct BlockConstrainedConfirmationStrategy<Client: ITaikoL1Client> {
    sender: ConfirmationSender<Client>,
    blocks: Arc<RwLock<Vec<Block>>>,
    max_blocks: usize,
    valid_anchor_id: Arc<RwLock<ValidAnchor<Client>>>,
    taiko_inbox: TaikoInboxInstance,
    valid_timestamp: ValidTimestamp,
}

impl<Client: ITaikoL1Client> BlockConstrainedConfirmationStrategy<Client> {
    pub const fn new(
        sender: ConfirmationSender<Client>,
        blocks: Arc<RwLock<Vec<Block>>>,
        max_blocks: usize,
        valid_anchor_id: Arc<RwLock<ValidAnchor<Client>>>,
        taiko_inbox: TaikoInboxInstance,
        valid_timestamp: ValidTimestamp,
    ) -> Self {
        Self {
            sender,
            blocks,
            max_blocks,
            valid_anchor_id,
            taiko_inbox,
            valid_timestamp,
        }
    }

    pub async fn send(&self, l1_slot_timestamp: u64, force_send: bool) -> ConfirmationResult<()> {
        info!("send force={force_send}");
        info!("blocks: {}", self.blocks.read().await.len());
        self.blocks.write().await.retain(|block| {
            self.valid_timestamp
                .check(l1_slot_timestamp, block.header.timestamp, 0, 0)
        });
        info!(
            "after removing outdated blocks: {}",
            self.blocks.read().await.len()
        );
        let blocks: Vec<Block> = self
            .blocks
            .read()
            .await
            .iter()
            .filter_map(|block| {
                if block.header.timestamp < l1_slot_timestamp - 12 {
                    Some(block.clone())
                } else {
                    None
                }
            })
            .collect();
        info!("after removing too new blocks: {}", blocks.len());
        if blocks.is_empty()
            || (blocks.len() < self.max_blocks
                && !force_send
                && !self.valid_anchor_id.read().await.is_valid_after(2))
        {
            return Ok(());
        }
        let number_of_blobs = 0u8;

        let mut txs = Vec::new();
        let mut block_params = Vec::new();
        let mut last_timestamp = blocks.first().expect("Must be present").header.timestamp;
        let first_anchor_tx = blocks
            .first()
            .expect("Must be present")
            .transactions
            .txns()
            .next();
        if first_anchor_tx.is_none() {
            error!("{:?}", blocks.first().expect("Must be present"));
            return Err(ConfirmationError::EmptyBlock(
                blocks.first().expect("Must be present").header.number,
            ));
        }
        let batch_anchor_id: u64 =
            get_anchor_block_id_from_bytes(first_anchor_tx.expect("Must be present").input())?;
        info!(
            "{:?}",
            blocks
                .iter()
                .map(|block| (block.header.number, block.header.timestamp))
                .collect::<Vec<(u64, u64)>>()
        );
        for block in blocks.into_iter() {
            info!(
                "block {} {} {}",
                block.header.number,
                block.header.timestamp,
                block.transactions.len()
            );
            let timestamp = block.header.timestamp;
            let number = block.header.number;
            debug!(
                "ts {} {} {}",
                timestamp,
                last_timestamp,
                timestamp - last_timestamp
            );
            if let Ok(time_shift) = (timestamp - last_timestamp).try_into() {
                let anchor_tx = block.transactions.txns().next().cloned();
                if let Some(anchor_tx) = anchor_tx {
                    let block_anchor_id = get_anchor_block_id_from_bytes(anchor_tx.input())?;
                    if block_anchor_id == batch_anchor_id {
                        let block_txs = get_tx_envelopes_without_anchor_from_block(block);
                        block_params.push(BlockParams {
                            numTransactions: block_txs.len() as u16,
                            timeShift: time_shift,
                            signalSlots: vec![],
                        });
                        last_timestamp = timestamp;
                        debug!("block txs: {:?}", block_txs);
                        compress(block_txs.clone())?;
                        txs.extend(block_txs.into_iter());
                    } else {
                        info!(
                            "Found new anchor id {block_anchor_id} for batch with anchor id {batch_anchor_id}. Splitting confirmation."
                        );
                        break;
                    }
                } else {
                    error!("{block:?}");
                    return Err(ConfirmationError::EmptyBlock(number));
                }
            } else {
                info!(
                    "Block {} is too far away from previous block. Splitting confirmation step.",
                    number
                );
                break;
            }
        }
        debug!("txs: {txs:?}");

        let parent_batch_meta_hash = get_latest_confirmed_batch(&self.taiko_inbox)
            .await
            .map(|batch| batch.metaHash)
            .unwrap_or_default();
        let tx_bytes = Bytes::from(compress(txs.clone())?);
        let blob_params = BlobParams {
            blobHashes: vec![],
            firstBlobIndex: 0,
            numBlobs: number_of_blobs,
            byteOffset: 0,
            byteSize: tx_bytes.len() as u32,
            createdIn: 0,
        };

        let propose_batch_params = create_propose_batch_params(
            self.sender.address(),
            block_params,
            blob_params,
            parent_batch_meta_hash,
            batch_anchor_id,
            last_timestamp,
            self.sender.address(),
        );

        debug!("params: {propose_batch_params:?}");
        debug!("tx_list: {tx_bytes:?}");
        let tx = TransactionRequest::default().with_call(&TaikoInbox::proposeBatchCall {
            _params: propose_batch_params,
            _txList: tx_bytes,
        });
        self.valid_anchor_id.write().await.update().await?;
        self.sender.send(tx).await?;
        Ok(())
    }
}

fn create_propose_batch_params(
    proposer: Address,
    blocks: Vec<BlockParams>,
    blob_params: BlobParams,
    parent_meta_hash: B256,
    anchor_block_id: u64,
    last_block_timestamp: u64,
    coinbase: Address,
) -> Bytes {
    let batch_params = BatchParams {
        proposer,
        coinbase,
        parentMetaHash: parent_meta_hash,
        anchorBlockId: anchor_block_id,
        lastBlockTimestamp: last_block_timestamp,
        revertIfNotFirstProposal: false,
        blobParams: blob_params,
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
