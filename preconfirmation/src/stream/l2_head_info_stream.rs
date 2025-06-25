use std::sync::Arc;

use alloy_consensus::Header;
use alloy_primitives::Bytes;
use alloy_rpc_types_eth::Block;
use async_stream::stream;
use futures::{Stream, pin_mut};
use tokio::sync::RwLock;
use tokio_stream::StreamExt;
use tracing::info;

use crate::{compression::compress, log_util::log_error, taiko::contracts::TaikoInbox};

enum StreamData {
    Unconfirmed(Box<Block>),
    Confirmed(Box<TaikoInbox::BatchProposed>),
}

pub async fn get_l2_head_stream<F, FnFut>(
    l2_block_stream: impl Stream<
        Item = Result<Block, alloy_json_rpc::RpcError<alloy_transport::TransportErrorKind>>,
    >,
    confirmed_header_stream: impl Stream<Item = alloy_sol_types::Result<TaikoInbox::BatchProposed>>,
    unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>>,
    get_header: F,
) -> impl Stream<Item = Header>
where
    FnFut: Future<Output = Result<Header, reqwest::Error>> + Send + 'static,
    F: Fn(u64) -> FnFut,
{
    stream! {
        let merged_stream = l2_block_stream.filter_map(|block| block.ok()).map(|block| StreamData::Unconfirmed(Box::new(block))).merge(
            confirmed_header_stream
            .filter_map(|result| result.ok())
            .map(|batch_proposed| StreamData::Confirmed(Box::new(batch_proposed))));

        let latest_batch_proposed: Arc<RwLock<Option<TaikoInbox::BatchProposed>>> = Arc::new(RwLock::new(None));
        pin_mut!(merged_stream);
        while let Some(stream_data) = merged_stream.next().await {
            match stream_data {
                StreamData::Unconfirmed(block) => {
                    info!("Received l2 block: {}", block.header.number);
                    if let Some(latest_batch_proposed) = latest_batch_proposed.read().await.clone() {
                        info!("latest batch proposed: {}", latest_batch_proposed.info.lastBlockId);
                        if block.header.number > latest_batch_proposed.info.lastBlockId {
                            let header = block.header.inner.clone();
                            unconfirmed_l2_blocks.write().await.push(*block);
                            yield header;
                        }
                    } else {
                        let header = block.header.inner.clone();
                        unconfirmed_l2_blocks.write().await.push(*block);
                        yield header;
                    }
                },
                StreamData::Confirmed(batch_proposed) => {
                    info!("Received confirmation: {}", batch_proposed.info.lastBlockId);
                    let info = {
                        let mut unconfirmed_l2_blocks = unconfirmed_l2_blocks.write().await;
                        let mut expected_confirmed_l2_blocks: Vec<_> = unconfirmed_l2_blocks.iter().filter(|block| block.header.number <= batch_proposed.info.lastBlockId).cloned().collect();
                        expected_confirmed_l2_blocks.sort_by(|a, b| a.header.number.cmp(&b.header.number));
                        let expected_number_of_blocks = expected_confirmed_l2_blocks.len();
                        let txs: Vec<_> = expected_confirmed_l2_blocks.into_iter().flat_map(|block| block.transactions.into_transactions_vec().into_iter().map(|tx| tx.inner.into_inner())).collect();
                        let tx_bytes = Bytes::from(compress(txs.clone()).unwrap());
                        if expected_number_of_blocks == batch_proposed.info.blocks.len() && tx_bytes == batch_proposed.txList {
                            info!("In sync with L2, cleanup confirmed blocks");
                            unconfirmed_l2_blocks.retain(|block| block.header.number > batch_proposed.info.lastBlockId);
                        } else {
                            info!("Out of sync with L2: blocks: {} vs {}", expected_number_of_blocks, batch_proposed.info.blocks.len());
                            info!("{:?}", tx_bytes);
                            info!("{:?}", batch_proposed.txList);
                            unconfirmed_l2_blocks.clear();
                        }
                        let info = if unconfirmed_l2_blocks.is_empty() {
                            log_error(get_header(batch_proposed.info.lastBlockId).await, "Failed to query last block")
                        } else {
                            None
                        };
                        *latest_batch_proposed.write().await = Some(*batch_proposed);
                        info
                    };
                    if let Some(info) = info {
                        yield info;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use alloy_consensus::{TxEnvelope, TxLegacy, TypedTransaction, transaction::Recovered};
    use alloy_primitives::{Address, B256, TxKind, U256, address};
    use alloy_rpc_types_eth::{BlockTransactions, Transaction, TransactionInfo};
    use async_stream::stream;
    use k256::ecdsa::SigningKey;
    use std::{pin::pin, sync::Arc};
    use tokio::sync::Notify;

    use crate::{
        encode_util::hex_decode,
        taiko::sign::{get_signing_key, sign_anchor_tx},
        test_util::get_header,
    };

    use super::*;

    const TEST_SIGNER_ADDRESS: Address = address!("0x1670090000000000000000000000000000010001");

    fn get_test_signing_key() -> SigningKey {
        get_signing_key("0x92954368afd3caa1f3ce3ead0069c1af414054aefe1ef9aeacc1bf426222ce38")
    }

    fn get_test_typed_transaction(nonce: u64) -> TypedTransaction {
        TypedTransaction::from(TxLegacy {
            chain_id: Some(167009u64),
            nonce,
            gas_price: 0u128,
            gas_limit: 0u64,
            to: TxKind::Call(TEST_SIGNER_ADDRESS),
            value: U256::default(),
            input: Bytes::default(),
        })
    }

    fn get_test_signed_transaction(nonce: u64) -> Transaction<TxEnvelope> {
        let tx = get_test_typed_transaction(nonce);
        let signing_key = get_test_signing_key();
        let signature = sign_anchor_tx(&signing_key, &tx).unwrap();
        let tx = TxEnvelope::new_unhashed(tx, signature);
        Transaction::from_transaction(
            Recovered::new_unchecked(tx, TEST_SIGNER_ADDRESS),
            TransactionInfo::default(),
        )
    }

    fn get_test_block(number: u64, timestamp: u64) -> Block {
        Block::empty(alloy_rpc_types_eth::Header::new(get_header(
            number, timestamp,
        )))
    }

    fn get_test_block_with_txs(
        number: u64,
        timestamp: u64,
        txs: Vec<Transaction<TxEnvelope>>,
    ) -> Block {
        Block::new(
            alloy_rpc_types_eth::Header::new(get_header(number, timestamp)),
            BlockTransactions::Full(txs),
        )
    }

    fn get_test_block_params(
        number_of_transactions: u16,
        time_shift: u8,
    ) -> TaikoInbox::BlockParams {
        TaikoInbox::BlockParams {
            numTransactions: number_of_transactions,
            timeShift: time_shift,
            signalSlots: vec![],
        }
    }

    fn get_test_batch_info(
        number: u64,
        timestamp: u64,
        blocks: Vec<TaikoInbox::BlockParams>,
    ) -> TaikoInbox::BatchInfo {
        TaikoInbox::BatchInfo {
            txsHash: B256::ZERO,
            blocks,
            blobHashes: vec![],
            extraData: B256::ZERO,
            coinbase: Address::random(),
            proposedIn: 0,
            blobCreatedIn: 0,
            blobByteOffset: 0,
            blobByteSize: 0,
            gasLimit: 0,
            lastBlockId: number,
            lastBlockTimestamp: timestamp,
            anchorBlockId: 0,
            anchorBlockHash: B256::ZERO,
            baseFeeConfig: TaikoInbox::BaseFeeConfig {
                adjustmentQuotient: 8,
                sharingPctg: 50,
                gasIssuancePerSecond: 5_000_000,
                minGasExcess: 1_344_899_430,
                maxGasIssuancePerBlock: 600_000_000,
            },
        }
    }

    fn get_test_batch_meta() -> TaikoInbox::BatchMetadata {
        TaikoInbox::BatchMetadata {
            infoHash: B256::ZERO,
            proposer: Address::random(),
            batchId: 0,
            proposedAt: 0,
        }
    }

    fn get_test_batch_proposed(number: u64, timestamp: u64) -> TaikoInbox::BatchProposed {
        TaikoInbox::BatchProposed {
            info: get_test_batch_info(number, timestamp, vec![]),
            meta: get_test_batch_meta(),
            txList: Bytes::copy_from_slice(&hex_decode("0x78da010100feffc000c100c1").unwrap()),
        }
    }

    fn get_test_batch_proposed_with_txs(
        number: u64,
        timestamp: u64,
        block_params: Vec<TaikoInbox::BlockParams>,
        txs: Vec<Transaction<TxEnvelope>>,
    ) -> TaikoInbox::BatchProposed {
        TaikoInbox::BatchProposed {
            info: get_test_batch_info(number, timestamp, block_params),
            meta: get_test_batch_meta(),
            txList: Bytes::copy_from_slice(
                &compress(txs.into_iter().map(|tx| tx.into_inner()).collect()).unwrap(),
            ),
        }
    }

    #[tokio::test]
    async fn header_info_stream_returns_info_from_l2_blocks_if_no_confirmed_blocks_are_yet_available()
     {
        let confirmation_stream_start = Arc::new(Notify::new());

        let confirmation_stream = pin!(stream! {
            confirmation_stream_start.notified().await;
            yield Ok(get_test_batch_proposed(1, 2));
        });
        let block1 = get_test_block(1, 3);
        let block2 = get_test_block(2, 5);
        let expected_unconfirmed_l2_blocks = vec![block1.clone(), block2.clone()];
        let l2_block_stream = pin!(stream! {
            yield Ok(block1);
            yield Ok(block2);
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: u64| async move { Ok(get_header(id, 0u64)) };
        let stream = get_l2_head_stream(
            l2_block_stream,
            confirmation_stream,
            unconfirmed_l2_blocks.clone(),
            f,
        )
        .await;
        pin_mut!(stream);
        let received = stream.next().await.unwrap();
        assert_eq!(received, get_header(1, 3));
        let received = stream.next().await.unwrap();
        assert_eq!(received, get_header(2, 5));
        assert_eq!(
            *unconfirmed_l2_blocks.read().await,
            expected_unconfirmed_l2_blocks
        );
    }

    #[tokio::test]
    async fn header_info_stream_returns_info_from_l2_blocks_only_if_block_number_is_bigger_than_confirmed_l1_block()
     {
        let l2_block_stream_start = Arc::new(Notify::new());
        let l2_block_stream_trigger = l2_block_stream_start.clone();

        let confirmation_stream = pin!(stream! {
            yield Ok(get_test_batch_proposed(2, 2));
            l2_block_stream_trigger.notify_one();
        });
        let block3 = get_test_block(3, 3);
        let expected_unconfirmed_l2_blocks = vec![block3.clone()];
        let l2_block_stream = pin!(stream! {
            l2_block_stream_start.notified().await;
            yield Ok(get_test_block(1, 3));
            yield Ok(get_test_block(2, 5));
            yield Ok(block3);
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: u64| async move { Ok(get_header(id, 0u64)) };
        let stream = get_l2_head_stream(
            l2_block_stream,
            confirmation_stream,
            unconfirmed_l2_blocks.clone(),
            f,
        )
        .await;
        pin_mut!(stream);
        let received = stream.next().await.unwrap();
        assert_eq!(received, get_header(2, 0));
        let received = stream.next().await.unwrap();
        assert_eq!(received, get_header(3, 3));
        let received = stream.next().await;
        assert!(received.is_none());
        assert_eq!(
            *unconfirmed_l2_blocks.read().await,
            expected_unconfirmed_l2_blocks
        );
    }

    #[tokio::test]
    async fn header_info_stream_removes_confirmed_blocks_without_txs_from_unconfirmed_blocks() {
        let confirmation_stream_start = Arc::new(Notify::new());
        let confirmation_stream_trigger = confirmation_stream_start.clone();

        let block_params = vec![get_test_block_params(0, 0), get_test_block_params(0, 1)];
        let confirmation_stream = pin!(stream! {
            confirmation_stream_start.notified().await;
            yield Ok(get_test_batch_proposed_with_txs(2, 2, block_params, vec![]));
        });
        let block1 = get_test_block(1, 3);
        let block2 = get_test_block(2, 5);
        let block3 = get_test_block(3, 3);
        let expected_unconfirmed_l2_blocks_before_confirmation =
            vec![block1.clone(), block2.clone(), block3.clone()];
        let final_expected_unconfirmed_l2_blocks = vec![block3.clone()];
        let l2_block_stream = pin!(stream! {
            yield Ok(block1);
            yield Ok(block2);
            yield Ok(block3);
            confirmation_stream_trigger.notify_one();
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: u64| async move { Ok(get_header(id, 0u64)) };
        let stream = get_l2_head_stream(
            l2_block_stream,
            confirmation_stream,
            unconfirmed_l2_blocks.clone(),
            f,
        )
        .await;
        pin_mut!(stream);

        let received = stream.next().await.unwrap();
        assert_eq!(received, get_header(1, 3));
        let received = stream.next().await.unwrap();
        assert_eq!(received, get_header(2, 5));
        let received = stream.next().await.unwrap();
        assert_eq!(received, get_header(3, 3));
        assert_eq!(
            *unconfirmed_l2_blocks.read().await,
            expected_unconfirmed_l2_blocks_before_confirmation
        );

        let received = stream.next().await;
        assert!(received.is_none());
        assert_eq!(
            *unconfirmed_l2_blocks.read().await,
            final_expected_unconfirmed_l2_blocks
        );
    }

    #[tokio::test]
    async fn header_info_stream_removes_one_confirmed_block_with_one_tx_from_unconfirmed_blocks() {
        let confirmation_stream_start = Arc::new(Notify::new());
        let confirmation_stream_trigger = confirmation_stream_start.clone();

        let txs = vec![get_test_signed_transaction(0)];
        let proposed_txs = txs.clone();
        let block_params = vec![get_test_block_params(1, 0)];
        let confirmation_stream = pin!(stream! {
            confirmation_stream_start.notified().await;
            yield Ok(get_test_batch_proposed_with_txs(1, 2, block_params, proposed_txs));
        });
        let block1 = get_test_block_with_txs(1, 3, txs);
        let block2 = get_test_block(2, 5);
        let expected_unconfirmed_l2_blocks_before_confirmation =
            vec![block1.clone(), block2.clone()];
        let final_expected_unconfirmed_l2_blocks = vec![block2.clone()];
        let l2_block_stream = pin!(stream! {
            yield Ok(block1);
            yield Ok(block2);
            confirmation_stream_trigger.notify_one();
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: u64| async move { Ok(get_header(id, 0u64)) };
        let stream = get_l2_head_stream(
            l2_block_stream,
            confirmation_stream,
            unconfirmed_l2_blocks.clone(),
            f,
        )
        .await;
        pin_mut!(stream);

        let received = stream.next().await.unwrap();
        assert_eq!(received, get_header(1, 3));
        let received = stream.next().await.unwrap();
        assert_eq!(received, get_header(2, 5));
        assert_eq!(
            *unconfirmed_l2_blocks.read().await,
            expected_unconfirmed_l2_blocks_before_confirmation
        );

        let received = stream.next().await;
        assert!(received.is_none());
        assert_eq!(
            *unconfirmed_l2_blocks.read().await,
            final_expected_unconfirmed_l2_blocks
        );
    }

    #[tokio::test]
    async fn header_info_stream_removes_all_blocks_from_unconfirmed_blocks_if_txs_cause_mismatch() {
        let confirmation_stream_start = Arc::new(Notify::new());
        let confirmation_stream_trigger = confirmation_stream_start.clone();

        let txs = vec![get_test_signed_transaction(0)];
        let proposed_txs = vec![get_test_signed_transaction(1)];
        let block_params = vec![get_test_block_params(1, 0)];
        let confirmation_stream = pin!(stream! {
            confirmation_stream_start.notified().await;
            yield Ok(get_test_batch_proposed_with_txs(1, 2, block_params, proposed_txs));
        });
        let block1 = get_test_block_with_txs(1, 3, txs);
        let block2 = get_test_block(2, 5);
        let expected_unconfirmed_l2_blocks_before_confirmation =
            vec![block1.clone(), block2.clone()];
        let final_expected_unconfirmed_l2_blocks = vec![];
        let l2_block_stream = pin!(stream! {
            yield Ok(block1);
            yield Ok(block2);
            confirmation_stream_trigger.notify_one();
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: u64| async move { Ok(get_header(id, 0u64)) };
        let stream = get_l2_head_stream(
            l2_block_stream,
            confirmation_stream,
            unconfirmed_l2_blocks.clone(),
            f,
        )
        .await;
        pin_mut!(stream);

        let received = stream.next().await.unwrap();
        assert_eq!(received, get_header(1, 3));
        let received = stream.next().await.unwrap();
        assert_eq!(received, get_header(2, 5));
        assert_eq!(
            *unconfirmed_l2_blocks.read().await,
            expected_unconfirmed_l2_blocks_before_confirmation
        );

        let received = stream.next().await.unwrap();
        assert_eq!(received, get_header(1, 0));
        assert_eq!(
            *unconfirmed_l2_blocks.read().await,
            final_expected_unconfirmed_l2_blocks
        );
    }

    #[tokio::test]
    async fn header_info_stream_removes_one_confirmed_block_with_two_txs_from_unconfirmed_blocks() {
        let confirmation_stream_start = Arc::new(Notify::new());
        let confirmation_stream_trigger = confirmation_stream_start.clone();

        let txs = vec![
            get_test_signed_transaction(0),
            get_test_signed_transaction(1),
        ];
        let proposed_txs = txs.clone();
        let block_params = vec![get_test_block_params(2, 0)];
        let confirmation_stream = pin!(stream! {
            confirmation_stream_start.notified().await;
            yield Ok(get_test_batch_proposed_with_txs(1, 2, block_params, proposed_txs));
        });
        let block1 = get_test_block_with_txs(1, 3, txs);
        let block2 = get_test_block(2, 5);
        let expected_unconfirmed_l2_blocks_before_confirmation =
            vec![block1.clone(), block2.clone()];
        let final_expected_unconfirmed_l2_blocks = vec![block2.clone()];
        let l2_block_stream = pin!(stream! {
            yield Ok(block1);
            yield Ok(block2);
            confirmation_stream_trigger.notify_one();
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: u64| async move { Ok(get_header(id, 0u64)) };
        let stream = get_l2_head_stream(
            l2_block_stream,
            confirmation_stream,
            unconfirmed_l2_blocks.clone(),
            f,
        )
        .await;
        pin_mut!(stream);

        let received = stream.next().await.unwrap();
        assert_eq!(received, get_header(1, 3));
        let received = stream.next().await.unwrap();
        assert_eq!(received, get_header(2, 5));
        assert_eq!(
            *unconfirmed_l2_blocks.read().await,
            expected_unconfirmed_l2_blocks_before_confirmation
        );

        let received = stream.next().await;
        assert!(received.is_none());
        assert_eq!(
            *unconfirmed_l2_blocks.read().await,
            final_expected_unconfirmed_l2_blocks
        );
    }

    #[tokio::test]
    async fn header_info_stream_removes_two_confirmed_blocks_with_two_txs_from_unconfirmed_blocks()
    {
        let confirmation_stream_start = Arc::new(Notify::new());
        let confirmation_stream_trigger = confirmation_stream_start.clone();

        let txs1 = vec![
            get_test_signed_transaction(0),
            get_test_signed_transaction(1),
        ];
        let txs2 = vec![
            get_test_signed_transaction(2),
            get_test_signed_transaction(3),
        ];
        let mut proposed_txs = txs1.clone();
        proposed_txs.extend(txs2.clone().into_iter());
        let block_params = vec![get_test_block_params(2, 0), get_test_block_params(2, 1)];
        let confirmation_stream = pin!(stream! {
            confirmation_stream_start.notified().await;
            yield Ok(get_test_batch_proposed_with_txs(2, 2, block_params, proposed_txs));
        });
        let block1 = get_test_block_with_txs(1, 3, txs1);
        let block2 = get_test_block_with_txs(2, 5, txs2);
        let block3 = get_test_block(3, 3);
        let expected_unconfirmed_l2_blocks_before_confirmation =
            vec![block1.clone(), block2.clone(), block3.clone()];
        let final_expected_unconfirmed_l2_blocks = vec![block3.clone()];
        let l2_block_stream = pin!(stream! {
            yield Ok(block1);
            yield Ok(block2);
            yield Ok(block3);
            confirmation_stream_trigger.notify_one();
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: u64| async move { Ok(get_header(id, 0u64)) };
        let stream = get_l2_head_stream(
            l2_block_stream,
            confirmation_stream,
            unconfirmed_l2_blocks.clone(),
            f,
        )
        .await;
        pin_mut!(stream);

        let received = stream.next().await.unwrap();
        assert_eq!(received, get_header(1, 3));
        let received = stream.next().await.unwrap();
        assert_eq!(received, get_header(2, 5));
        let received = stream.next().await.unwrap();
        assert_eq!(received, get_header(3, 3));
        assert_eq!(
            *unconfirmed_l2_blocks.read().await,
            expected_unconfirmed_l2_blocks_before_confirmation
        );

        let received = stream.next().await;
        assert!(received.is_none());
        assert_eq!(
            *unconfirmed_l2_blocks.read().await,
            final_expected_unconfirmed_l2_blocks
        );
    }

    #[tokio::test]
    async fn header_info_stream_removes_all_unconfirmed_blocks_if_l2_and_l1_are_out_of_sync_due_to_different_amount_of_blocks()
     {
        let confirmation_stream_start = Arc::new(Notify::new());
        let confirmation_stream_trigger = confirmation_stream_start.clone();

        let block_params = vec![get_test_block_params(0, 0)];
        let confirmation_stream = pin!(stream! {
            confirmation_stream_start.notified().await;
            yield Ok(get_test_batch_proposed_with_txs(2, 2, block_params, vec![]));
        });
        let block1 = get_test_block(1, 3);
        let block2 = get_test_block(2, 5);
        let block3 = get_test_block(3, 3);
        let expected_unconfirmed_l2_blocks_before_confirmation =
            vec![block1.clone(), block2.clone(), block3.clone()];
        let l2_block_stream = pin!(stream! {
            yield Ok(block1);
            yield Ok(block2);
            yield Ok(block3);
            confirmation_stream_trigger.notify_one();
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: u64| async move { Ok(get_header(id, 0u64)) };
        let stream = get_l2_head_stream(
            l2_block_stream,
            confirmation_stream,
            unconfirmed_l2_blocks.clone(),
            f,
        )
        .await;
        pin_mut!(stream);

        let received = stream.next().await.unwrap();
        assert_eq!(received, get_header(1, 3));
        let received = stream.next().await.unwrap();
        assert_eq!(received, get_header(2, 5));
        let received = stream.next().await.unwrap();
        assert_eq!(received, get_header(3, 3));
        assert_eq!(
            *unconfirmed_l2_blocks.read().await,
            expected_unconfirmed_l2_blocks_before_confirmation
        );

        let received = stream.next().await.unwrap();
        assert_eq!(received, get_header(2, 0));
        assert_eq!(*unconfirmed_l2_blocks.read().await, vec![]);
    }
}
