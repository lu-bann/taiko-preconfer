use std::thread::JoinHandle;

use alloy_consensus::{BlobTransactionSidecar, Header, Transaction, TxEnvelope};
use alloy_eips::{BlockNumberOrTag, eip4844::env_settings::EnvKzgSettings};
use alloy_network::{EthereumWallet, TransactionBuilder, TransactionBuilder4844};
use alloy_primitives::{Address, Bytes, FixedBytes};
use alloy_provider::{
    Identity, Provider, RootProvider,
    fillers::{
        BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller, WalletFiller,
    },
    utils::Eip1559Estimation,
};
use alloy_rpc_types::TransactionRequest;
use alloy_rpc_types_eth::Block;
use alloy_sol_types::SolType;
use libdeflater::CompressionError;
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};
use thiserror::Error;
use tokio::{join, sync::RwLock};
use tracing::{debug, error, info};

use crate::{
    blob::{BlobEncodeError, tx_bytes_to_sidecar},
    compression::compress,
    taiko::contracts::{
        BatchParams, BlobParams, BlockParams, ProposeBatchParams, TaikoInbox, TaikoInboxInstance,
    },
    util::{
        get_anchor_block_id_from_bytes, get_tx_envelopes_without_anchor_from_block, log_error,
        now_as_millis,
    },
    verification::{TaikoInboxError, get_latest_confirmed_batch},
};

pub type TaikoProvider = FillProvider<
    JoinFill<
        JoinFill<
            Identity,
            JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>,
        >,
        WalletFiller<EthereumWallet>,
    >,
    RootProvider,
>;

#[derive(Debug, Error)]
pub enum TaikoL1ClientError {
    #[error("{0}")]
    Rpc(String),

    #[error("{0}")]
    BlobEncode(#[from] BlobEncodeError),

    #[error("{0}")]
    Compression(#[from] CompressionError),

    #[error("{0}")]
    TaikoInbox(#[from] TaikoInboxError),

    #[error("{0}")]
    Contract(#[from] alloy_contract::Error),

    #[error("{0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("{0}")]
    IO(#[from] std::io::Error),

    #[error("{0}")]
    FromUInt128(#[from] alloy_primitives::ruint::FromUintError<u128>),

    #[error("{0}")]
    PendingTransaction(#[from] alloy_provider::PendingTransactionError),
}

impl From<alloy_json_rpc::RpcError<alloy_transport::TransportErrorKind>> for TaikoL1ClientError {
    fn from(err: alloy_json_rpc::RpcError<alloy_transport::TransportErrorKind>) -> Self {
        Self::Rpc(crate::util::parse_transport_error(err))
    }
}

pub type TaikoL1ClientResult<T> = Result<T, TaikoL1ClientError>;

#[cfg_attr(test, mockall::automock)]
pub trait ITaikoL1Client {
    fn get_nonce(&self, address: Address) -> impl Future<Output = TaikoL1ClientResult<u64>>;

    fn get_blob_base_fee(&self) -> impl Future<Output = TaikoL1ClientResult<u128>>;

    fn get_header(&self, id: u64) -> impl Future<Output = TaikoL1ClientResult<Header>>;

    fn get_latest_header(&self) -> impl Future<Output = TaikoL1ClientResult<Header>>;

    fn estimate_gas(
        &self,
        tx: TransactionRequest,
    ) -> impl Future<Output = TaikoL1ClientResult<u64>>;

    fn estimate_eip1559_fees(&self)
    -> impl Future<Output = TaikoL1ClientResult<Eip1559Estimation>>;

    fn send(
        &self,
        blocks: Vec<Block>,
        anchor_id: u64,
    ) -> impl Future<Output = TaikoL1ClientResult<()>>;
}

#[derive(Debug, Clone)]
pub struct TaikoL1Client {
    provider: TaikoProvider,
    propose_timeout: Duration,
    tx_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
    preconfer_address: Address,
    preconf_router_address: Address,
    taiko_inbox: TaikoInboxInstance,
    use_blobs: bool,
}

impl TaikoL1Client {
    pub fn new(
        provider: TaikoProvider,
        propose_timeout: Duration,
        preconfer_address: Address,
        preconf_router_address: Address,
        taiko_inbox: TaikoInboxInstance,
        use_blobs: bool,
    ) -> Self {
        Self {
            provider,
            propose_timeout,
            tx_handle: Arc::new(None.into()),
            preconfer_address,
            preconf_router_address,
            taiko_inbox,
            use_blobs,
        }
    }
}

impl ITaikoL1Client for TaikoL1Client {
    async fn get_nonce(&self, address: Address) -> TaikoL1ClientResult<u64> {
        Ok(self.provider.get_transaction_count(address).await?)
    }

    async fn get_blob_base_fee(&self) -> TaikoL1ClientResult<u128> {
        Ok(self.provider.get_blob_base_fee().await?)
    }

    async fn get_header(&self, id: u64) -> TaikoL1ClientResult<Header> {
        Ok(self
            .provider
            .get_block_by_number(BlockNumberOrTag::Number(id))
            .await?
            .ok_or(alloy_json_rpc::RpcError::NullResp)?
            .header
            .inner)
    }

    async fn get_latest_header(&self) -> TaikoL1ClientResult<Header> {
        Ok(self
            .provider
            .get_block_by_number(BlockNumberOrTag::Latest)
            .await?
            .ok_or(alloy_json_rpc::RpcError::NullResp)?
            .header
            .inner)
    }

    async fn estimate_gas(&self, tx: TransactionRequest) -> TaikoL1ClientResult<u64> {
        Ok(self.provider.estimate_gas(tx).await?)
    }

    async fn estimate_eip1559_fees(&self) -> TaikoL1ClientResult<Eip1559Estimation> {
        Ok(self.provider.estimate_eip1559_fees().await?)
    }

    async fn send(&self, blocks: Vec<Block>, anchor_id: u64) -> TaikoL1ClientResult<()> {
        info!("Send tx");
        let previous_tx_returned = self
            .tx_handle
            .read()
            .await
            .as_ref()
            .map(|handle| handle.is_finished())
            .unwrap_or(true);
        if !previous_tx_returned {
            info!("Previous transaction did not finish. Skipping tx.");
            return Ok(());
        }
        let provider = self.provider.clone();
        let timeout = self.propose_timeout;
        let preconfer_address = self.preconfer_address;
        let router_address = self.preconf_router_address;
        let taiko_inbox = self.taiko_inbox.clone();
        let use_blobs = self.use_blobs;

        *self.tx_handle.write().await = Some(
            std::thread::Builder::new()
                .stack_size(8 * 1024 * 1024)
                .spawn(move || {
                    let rt = tokio::runtime::Runtime::new()
                        .expect("Failed to get tokio runtime for sending confirmation.");
                    rt.block_on(async move {
                        let settings = EnvKzgSettings::Default.get();

                        let mut txs = Vec::new();
                        let mut block_params = Vec::new();

                        let last_timestamp =
                            read_blocks(blocks, anchor_id, &mut block_params, &mut txs);

                        let parent_batch_meta_hash = get_latest_confirmed_batch(&taiko_inbox)
                            .await
                            .map(|batch| batch.metaHash)
                            .unwrap_or_default();
                        let tx_bytes =
                            Bytes::from(compress(txs.clone()).expect("Failed to compress txs"));
                        debug!("tx_list: {tx_bytes:?}");
                        let tx_bytes_len = tx_bytes.len();
                        info!("tx bytes: {}", tx_bytes_len);
                        let (tx_list, sidecar) = if use_blobs {
                            (
                                Bytes::default(),
                                Some(
                                    tx_bytes_to_sidecar(tx_bytes, settings)
                                        .expect("Failed to get sidecar from bytes"),
                                ),
                            )
                        } else {
                            (tx_bytes, None)
                        };
                        let blob_params = get_blob_params(&sidecar, tx_bytes_len);
                        info!("blob params: {:?}", blob_params);

                        let propose_batch_params = create_propose_batch_params(
                            preconfer_address,
                            block_params,
                            blob_params,
                            parent_batch_meta_hash,
                            anchor_id,
                            last_timestamp,
                            preconfer_address,
                        );

                        debug!("params: {propose_batch_params:?}");
                        info!("send tx {}", now_as_millis());
                        let mut tx = TransactionRequest::default()
                            .with_from(preconfer_address)
                            .with_to(router_address)
                            .with_call(&TaikoInbox::proposeBatchCall {
                                _params: propose_batch_params,
                                _txList: tx_list,
                            });
                        if let Some(sidecar) = sidecar {
                            tx.set_blob_sidecar(sidecar);
                            info!("Set blob base fee");
                            if let Some(blob_base_fee) = log_error(
                                provider.get_blob_base_fee().await,
                                "Failed to get blob base fee",
                            ) {
                                tx = tx.max_fee_per_blob_gas(blob_base_fee);
                            }
                        }
                        info!("tx: {:?}", tx);
                        let (nonce, gas_limit, fee_estimate) = join!(
                            provider.get_transaction_count(preconfer_address),
                            provider.estimate_gas(tx.clone()),
                            provider.estimate_eip1559_fees(),
                        );

                        let gas_limit = log_error(
                            gas_limit.map_err(TaikoL1ClientError::from),
                            "Failed to estimate gas",
                        );
                        if gas_limit.is_none() {
                            return;
                        }

                        let fee_estimate = log_error(
                            fee_estimate.map_err(TaikoL1ClientError::from),
                            "Failed to estimate fee",
                        );
                        if fee_estimate.is_none() {
                            return;
                        }
                        let gas_limit = gas_limit.expect("Must be present");
                        let fee_estimate = fee_estimate.expect("Must be present");
                        let nonce = nonce.expect("Must be present");
                        let tx = tx
                            .with_gas_limit(gas_limit)
                            .with_max_fee_per_gas(fee_estimate.max_fee_per_gas * 12 / 10)
                            .with_max_priority_fee_per_gas(
                                fee_estimate.max_priority_fee_per_gas * 12 / 10,
                            )
                            .nonce(nonce);

                        info!("propose batch tx {tx:?}");

                        if let Some(tx_builder) = log_error(
                            provider.send_transaction(tx).await,
                            "Failed to get transaction builder",
                        ) {
                            let start = SystemTime::now();
                            if let Some(receipt) = log_error(
                                tx_builder
                                    .with_required_confirmations(2)
                                    .with_timeout(Some(timeout))
                                    .get_receipt()
                                    .await,
                                "Failed to send transaction",
                            ) {
                                let end = SystemTime::now();
                                let elapsed = end
                                    .duration_since(start)
                                    .expect("time went backwards during tx")
                                    .as_millis();
                                info!("receipt: {receipt:?}, elapsed={elapsed} ms");
                            }
                        }
                    });
                })?,
        );
        Ok(())
    }
}

fn get_blob_params(sidecar: &Option<BlobTransactionSidecar>, tx_bytes_len: usize) -> BlobParams {
    BlobParams {
        blobHashes: vec![],
        firstBlobIndex: 0,
        numBlobs: sidecar
            .as_ref()
            .map(|sidecar: &BlobTransactionSidecar| sidecar.blobs.len() as u8)
            .unwrap_or_default(),
        byteOffset: 0,
        byteSize: tx_bytes_len as u32,
        createdIn: 0,
    }
}

fn read_blocks(
    blocks: Vec<Block>,
    batch_anchor_id: u64,
    block_params: &mut Vec<BlockParams>,
    txs: &mut Vec<TxEnvelope>,
) -> u64 {
    let mut last_timestamp = blocks.first().expect("Must be present").header.timestamp;
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
                let block_anchor_id =
                    get_anchor_block_id_from_bytes(anchor_tx.input()).expect("Missing anchor id");
                if block_anchor_id == batch_anchor_id {
                    let block_txs = get_tx_envelopes_without_anchor_from_block(block);
                    block_params.push(BlockParams {
                        numTransactions: block_txs.len() as u16,
                        timeShift: time_shift,
                        signalSlots: vec![],
                    });
                    last_timestamp = timestamp;
                    debug!("block txs: {:?}", block_txs);
                    txs.extend(block_txs.into_iter());
                } else {
                    info!(
                        "Found new anchor id {block_anchor_id} for batch with anchor id {batch_anchor_id}. Splitting confirmation."
                    );
                    break;
                }
            } else {
                error!(
                    "Missing anchor transaction in block {}",
                    block.header.number
                );
                error!("{block:?}");
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

    last_timestamp
}

pub fn create_propose_batch_params(
    proposer: Address,
    blocks: Vec<BlockParams>,
    blob_params: BlobParams,
    parent_meta_hash: FixedBytes<32>,
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
