use std::{cell::RefCell, num::TryFromIntError};

use alloy_primitives::{Address, B256, Bytes};
use alloy_provider::network::TransactionBuilder as _;
use alloy_rpc_types::TransactionRequest;
use alloy_signer_local::LocalSigner;
use k256::ecdsa::SigningKey;
use thiserror::Error;
use tokio::join;
use tracing::{debug, error, info};

use crate::{
    compression::compress,
    taiko::{
        contracts::taiko_wrapper::{BlockParams, TaikoWrapper},
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
    TryU8FromU64(#[from] TryFromIntError),

    #[error("{0}")]
    TaikoL1Client(#[from] crate::taiko::taiko_l1_client::TaikoL1ClientError),
}

pub type ConfirmationResult<T> = Result<T, ConfirmationError>;

#[derive(Debug)]
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
        }
        let tx = tx
            .with_gas_limit(gas_limit?)
            .with_max_fee_per_gas(fee_estimate.max_fee_per_gas)
            .with_max_priority_fee_per_gas(fee_estimate.max_priority_fee_per_gas)
            .nonce(nonce?);

        info!("propose batch tx {tx:?}");
        self.client.send(tx).await?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct InstantConfirmationStrategy<Client: ITaikoL1Client> {
    sender: ConfirmationSender<Client>,
}

impl<Client: ITaikoL1Client> InstantConfirmationStrategy<Client> {
    pub const fn new(sender: ConfirmationSender<Client>) -> Self {
        Self { sender }
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
            self.sender.address(),
            tx_bytes.len(),
            block_params,
            parent_meta_hash,
            block.anchor_block_id,
            block.last_block_timestamp,
            self.sender.address(),
            number_of_blobs,
        );

        println!("params");
        println!("{:?}", propose_batch_params);
        println!("tx_list");
        println!("{:?}", tx_bytes);
        let tx = TransactionRequest::default().with_call(&TaikoWrapper::proposeBatchCall {
            _params: propose_batch_params,
            _txList: tx_bytes,
        });
        self.sender.send(tx).await
    }
}

#[derive(Debug)]
pub struct BlockConstrainedConfirmationStrategy<Client: ITaikoL1Client> {
    sender: ConfirmationSender<Client>,
    blocks: RefCell<Vec<SimpleBlock>>,
    max_blocks: usize,
}

impl<Client: ITaikoL1Client> BlockConstrainedConfirmationStrategy<Client> {
    pub const fn new(sender: ConfirmationSender<Client>, max_blocks: usize) -> Self {
        Self {
            sender,
            blocks: RefCell::new(vec![]),
            max_blocks,
        }
    }

    pub async fn confirm(&self, block: SimpleBlock) -> ConfirmationResult<()> {
        self.blocks.borrow_mut().push(block.clone());
        if self.blocks.borrow().len() < self.max_blocks {
            return Ok(());
        }
        self.send().await
    }

    pub async fn send(&self) -> ConfirmationResult<()> {
        if self.blocks.borrow().is_empty() {
            return Ok(());
        }
        let blocks = self.blocks.borrow().clone();
        debug!("Compression");
        let number_of_blobs = 0u8;

        let mut txs = Vec::new();
        let mut block_params = Vec::new();
        let mut last_timestamp = blocks.first().unwrap().header.timestamp;
        let mut last_l1_anchor_block_id = 0;
        let mut last_l1_block_timestamp = 0;
        for block in blocks.into_iter() {
            let tx_len = block.txs.len() as u16;
            txs.extend(block.txs.into_iter());
            block_params.push(BlockParams {
                numTransactions: tx_len,
                timeShift: (block.header.timestamp - last_timestamp).try_into()?,
                signalSlots: vec![],
            });
            last_timestamp = block.header.timestamp;
            last_l1_anchor_block_id = std::cmp::max(last_l1_anchor_block_id, block.anchor_block_id);
            last_l1_block_timestamp =
                std::cmp::max(last_l1_block_timestamp, block.last_block_timestamp);
        }

        let parent_meta_hash = B256::ZERO;
        let tx_bytes = Bytes::from(compress(txs.clone())?);
        info!("Create propose batch params");
        let propose_batch_params = create_propose_batch_params(
            self.sender.address(),
            tx_bytes.len(),
            block_params,
            parent_meta_hash,
            last_l1_anchor_block_id,
            last_l1_block_timestamp,
            self.sender.address(),
            number_of_blobs,
        );

        println!("params");
        println!("{:?}", propose_batch_params);
        println!("tx_list");
        println!("{:?}", tx_bytes);
        let tx = TransactionRequest::default().with_call(&TaikoWrapper::proposeBatchCall {
            _params: propose_batch_params,
            _txList: tx_bytes,
        });
        self.sender.send(tx).await?;
        let _ = self.blocks.take();
        Ok(())
    }
}
