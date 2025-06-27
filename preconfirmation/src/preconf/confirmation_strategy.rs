use std::{num::TryFromIntError, sync::Arc};

use alloy_primitives::{Address, Bytes};
use alloy_provider::network::TransactionBuilder as _;
use alloy_rpc_types::TransactionRequest;
use alloy_rpc_types_eth::Block;
use alloy_signer_local::LocalSigner;
use k256::ecdsa::SigningKey;
use thiserror::Error;
use tokio::{join, sync::RwLock};
use tracing::{debug, error, info};

use crate::{
    compression::compress,
    taiko::{
        anchor::ValidAnchorId,
        contracts::{
            TaikoInboxInstance,
            taiko_wrapper::{BlockParams, TaikoWrapper},
        },
        propose_batch::create_propose_batch_params,
        taiko_l1_client::ITaikoL1Client,
    },
    util::get_tx_envelopes_from_block,
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
    TryU8FromU64(#[from] TryFromIntError),

    #[error("{0}")]
    TaikoL1Client(#[from] crate::taiko::taiko_l1_client::TaikoL1ClientError),
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
        let signer_str = self.signer.address().to_string();
        let (nonce, gas_limit, fee_estimate) = join!(
            self.client.get_nonce(&signer_str),
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
    valid_anchor_id: Arc<RwLock<ValidAnchorId>>,
    taiko_inbox: TaikoInboxInstance,
}

impl<Client: ITaikoL1Client> BlockConstrainedConfirmationStrategy<Client> {
    pub const fn new(
        sender: ConfirmationSender<Client>,
        blocks: Arc<RwLock<Vec<Block>>>,
        max_blocks: usize,
        valid_anchor_id: Arc<RwLock<ValidAnchorId>>,
        taiko_inbox: TaikoInboxInstance,
    ) -> Self {
        Self {
            sender,
            blocks,
            max_blocks,
            valid_anchor_id,
            taiko_inbox,
        }
    }

    pub async fn send(&self, force_send: bool) -> ConfirmationResult<()> {
        info!("send force={force_send}");
        info!("blocks: {}", self.blocks.read().await.len());
        if self.blocks.read().await.is_empty()
            || (self.blocks.read().await.len() < self.max_blocks
                && !force_send
                && self.valid_anchor_id.read().await.is_valid_after(2))
        {
            return Ok(());
        }
        let blocks = self.blocks.read().await.clone();
        info!("Compression");
        let number_of_blobs = 0u8;

        let mut txs = Vec::new();
        let mut block_params = Vec::new();
        let mut last_timestamp = blocks.first().unwrap().header.timestamp;
        info!("Get txs from blocks");
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
            let tx_len = block.transactions.len() as u16;
            let timestamp = block.header.timestamp;
            let number = block.header.number;
            info!("read txs");
            info!(
                "ts {} {} {}",
                timestamp,
                last_timestamp,
                timestamp - last_timestamp
            );
            if let Ok(time_shift) = (timestamp - last_timestamp).try_into() {
                block_params.push(BlockParams {
                    numTransactions: tx_len,
                    timeShift: time_shift,
                    signalSlots: vec![],
                });
                last_timestamp = timestamp;
                txs.extend(get_tx_envelopes_from_block(block).into_iter());
            } else {
                info!(
                    "Block {} is too far away from previous block. Splitting confirmation step.",
                    number
                );
                break;
            }
        }
        info!("txs: {txs:?}");

        info!("get anchor id");
        let anchor_block_id = self.valid_anchor_id.write().await.get_and_update();
        info!("anchor {anchor_block_id}");
        let batch = get_latest_confirmed_batch(&self.taiko_inbox)
            .await
            .expect("last batch should be here");
        let tx_bytes = Bytes::from(compress(txs.clone())?);
        info!("Create propose batch params");
        let propose_batch_params = create_propose_batch_params(
            self.sender.address(),
            tx_bytes.len(),
            block_params,
            batch.metaHash,
            anchor_block_id,
            last_timestamp,
            self.sender.address(),
            number_of_blobs,
        );

        println!("params");
        println!("{propose_batch_params:?}");
        println!("tx_list");
        println!("{tx_bytes:?}");
        let tx = TransactionRequest::default().with_call(&TaikoWrapper::proposeBatchCall {
            _params: propose_batch_params,
            _txList: tx_bytes,
        });
        self.sender.send(tx).await?;
        Ok(())
    }
}
