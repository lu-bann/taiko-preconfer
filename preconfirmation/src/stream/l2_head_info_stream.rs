use std::sync::Arc;

use alloy_consensus::Header;
use alloy_rpc_types_eth::Block;
use async_stream::stream;
use futures::{Stream, future::BoxFuture, pin_mut};
use tokio::sync::RwLock;
use tokio_stream::StreamExt;
use tracing::{info, warn};

use crate::{util::log_error, verification::ILastBatchVerifier};

enum StreamData {
    Unconfirmed(Box<Block>),
    Confirmed(u64),
}

async fn update_unconfirmed_blocks(block: Block, unconfirmed_l2_blocks: &Arc<RwLock<Vec<Block>>>) {
    let mut unconfirmed = unconfirmed_l2_blocks.write().await;
    if let Some(current) = unconfirmed
        .iter_mut()
        .find(|unconfirmed_block| unconfirmed_block.header.number == block.header.number)
    {
        info!("Replace current block");
        *current = block;
    } else {
        info!("Add block");
        unconfirmed.push(block);
    }
    unconfirmed.sort_by(|a, b| a.header.number.cmp(&b.header.number));
}

pub async fn get_l2_head_stream<F, FnFut, Verifier>(
    l2_block_stream: impl Stream<Item = Block>,
    confirmed_last_block_id_stream: impl Stream<Item = u64>,
    unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>>,
    get_header: F,
    initial_confirmed_block_id: Option<u64>,
    verifier: Verifier,
) -> impl Stream<Item = Result<Header, crate::verification::TaikoInboxError>>
where
    FnFut: Future<Output = Result<Header, reqwest::Error>> + Send + 'static,
    F: Fn(u64) -> FnFut,
    Verifier: ILastBatchVerifier,
{
    stream! {
        let merged_stream = l2_block_stream.map(|block| StreamData::Unconfirmed(Box::new(block))).merge(
            confirmed_last_block_id_stream
            .map(StreamData::Confirmed));

        let last_confirmed_block_id: Arc<RwLock<Option<u64>>> = Arc::new(RwLock::new(initial_confirmed_block_id));
        pin_mut!(merged_stream);
        while let Some(stream_data) = merged_stream.next().await {
            match stream_data {
                StreamData::Unconfirmed(block) => {
                    info!("Received l2 block: {}", block.header.number);
                    if let Some(last_confirmed_block_id) = *last_confirmed_block_id.read().await {
                        info!("last confirmed block: {last_confirmed_block_id}");
                        if block.header.number >= last_confirmed_block_id {
                            let header = block.header.inner.clone();
                            if block.header.number > last_confirmed_block_id {
                                update_unconfirmed_blocks(*block, &unconfirmed_l2_blocks).await;
                            }
                            yield Ok(header);
                        }
                    } else {
                        let header = block.header.inner.clone();
                        update_unconfirmed_blocks(*block, &unconfirmed_l2_blocks).await;
                        yield Ok(header);
                    }
                },
                StreamData::Confirmed(confirmed_block_id) => {
                    info!("Received confirmation: {}, last: {:?}", confirmed_block_id, last_confirmed_block_id.read().await);
                    if confirmed_block_id > last_confirmed_block_id.read().await.unwrap_or_default() {
                        let confirmation_successful = verifier.verify(unconfirmed_l2_blocks.read().await.clone()).await?;
                        let header = if confirmation_successful {
                            info!("L1 and L2 state in sync. Last confirmed block {confirmed_block_id}.");
                            unconfirmed_l2_blocks.write().await.retain(|block| block.header.number > confirmed_block_id);
                            None
                        } else {
                            warn!("L1 and L2 state out of sync. Last confirmed block {confirmed_block_id}.");
                            unconfirmed_l2_blocks.write().await.clear();
                            log_error(get_header(confirmed_block_id).await, "Failed to query last block")
                        };

                        *last_confirmed_block_id.write().await = Some(confirmed_block_id);
                        if let Some(header) = header {
                            yield Ok(header);
                        }
                    }
                }
            }
        }
    }
}

pub async fn stream_l2_headers<
    'a,
    Value: Clone,
    E: std::convert::From<crate::verification::TaikoInboxError>,
    T: Fn(Header, Value) -> BoxFuture<'a, Result<(), E>>,
>(
    stream: impl Stream<Item = Result<Header, crate::verification::TaikoInboxError>>,
    f: T,
    current: Value,
) -> Result<(), E> {
    pin_mut!(stream);
    while let Some(header) = stream.next().await {
        f(header?, current.clone()).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use alloy_consensus::{TxEnvelope, TxLegacy, TypedTransaction, transaction::Recovered};
    use alloy_primitives::{Address, Bytes, TxKind, U256, address};
    use alloy_rpc_types_eth::{BlockTransactions, Transaction, TransactionInfo};
    use async_stream::stream;
    use k256::ecdsa::SigningKey;
    use std::{pin::pin, sync::Arc};
    use tokio::sync::Notify;

    use crate::{
        taiko::sign::{get_signing_key, sign_anchor_tx},
        test_util::get_header,
        verification::MockILastBatchVerifier,
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

    #[tokio::test]
    async fn header_info_stream_returns_info_from_l2_blocks_if_no_confirmed_blocks_are_yet_available()
     {
        let confirmation_stream_start = Arc::new(Notify::new());

        let confirmation_stream = pin!(stream! {
            confirmation_stream_start.notified().await;
            yield 1;
        });
        let block1 = get_test_block(1, 3);
        let block2 = get_test_block(2, 5);
        let expected_unconfirmed_l2_blocks = vec![block1.clone(), block2.clone()];
        let l2_block_stream = pin!(stream! {
            yield block1;
            yield block2;
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: u64| async move { Ok(get_header(id, 0u64)) };
        let mut verifier = MockILastBatchVerifier::new();
        verifier.expect_verify().never();
        let stream = get_l2_head_stream(
            l2_block_stream,
            confirmation_stream,
            unconfirmed_l2_blocks.clone(),
            f,
            None,
            verifier,
        )
        .await;
        pin_mut!(stream);
        let received = stream.next().await.unwrap().unwrap();
        assert_eq!(received, get_header(1, 3));
        let received = stream.next().await.unwrap().unwrap();
        assert_eq!(received, get_header(2, 5));
        assert_eq!(
            *unconfirmed_l2_blocks.read().await,
            expected_unconfirmed_l2_blocks
        );
    }

    #[tokio::test]
    async fn header_info_stream_replaces_updated_l2_blocks_in_unconfirmed_blocks() {
        let confirmation_stream_start = Arc::new(Notify::new());

        let confirmation_stream = pin!(stream! {
            confirmation_stream_start.notified().await;
            yield 1;
        });
        let block1 = get_test_block(1, 3);
        let block2 = get_test_block(1, 5);
        let expected_unconfirmed_l2_blocks = vec![block2.clone()];
        let l2_block_stream = pin!(stream! {
            yield block1;
            yield block2;
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: u64| async move { Ok(get_header(id, 0u64)) };
        let mut verifier = MockILastBatchVerifier::new();
        verifier.expect_verify().never();
        let stream = get_l2_head_stream(
            l2_block_stream,
            confirmation_stream,
            unconfirmed_l2_blocks.clone(),
            f,
            None,
            verifier,
        )
        .await;
        pin_mut!(stream);
        let received = stream.next().await.unwrap().unwrap();
        assert_eq!(received, get_header(1, 3));
        let received = stream.next().await.unwrap().unwrap();
        assert_eq!(received, get_header(1, 5));
        assert_eq!(
            *unconfirmed_l2_blocks.read().await,
            expected_unconfirmed_l2_blocks
        );
    }

    #[tokio::test]
    async fn header_info_stream_replaces_updated_l2_blocks_in_unconfirmed_blocks_with_confirmed_blocks_set()
     {
        let confirmation_stream_start = Arc::new(Notify::new());

        let confirmation_stream = pin!(stream! {
            confirmation_stream_start.notified().await;
            yield 1;
        });
        let block1 = get_test_block(1, 3);
        let block2 = get_test_block(1, 5);
        let expected_unconfirmed_l2_blocks = vec![];
        let l2_block_stream = pin!(stream! {
            yield block1;
            yield block2;
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: u64| async move { Ok(get_header(id, 0u64)) };
        let mut verifier = MockILastBatchVerifier::new();
        verifier.expect_verify().never();
        let stream = get_l2_head_stream(
            l2_block_stream,
            confirmation_stream,
            unconfirmed_l2_blocks.clone(),
            f,
            Some(1),
            verifier,
        )
        .await;
        pin_mut!(stream);
        let received = stream.next().await.unwrap().unwrap();
        assert_eq!(received, get_header(1, 3));
        let received = stream.next().await.unwrap().unwrap();
        assert_eq!(received, get_header(1, 5));
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
            yield 2;
            l2_block_stream_trigger.notify_one();
        });
        let block3 = get_test_block(3, 3);
        let expected_unconfirmed_l2_blocks = vec![block3.clone()];
        let l2_block_stream = pin!(stream! {
            l2_block_stream_start.notified().await;
            yield get_test_block(1, 3);
            yield get_test_block(2, 5);
            yield block3;
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: u64| async move { Ok(get_header(id, 0u64)) };
        let mut verifier = MockILastBatchVerifier::new();
        verifier.expect_verify().never();
        let stream = get_l2_head_stream(
            l2_block_stream,
            confirmation_stream,
            unconfirmed_l2_blocks.clone(),
            f,
            Some(2),
            verifier,
        )
        .await;
        pin_mut!(stream);
        let received = stream.next().await.unwrap().unwrap();
        assert_eq!(received, get_header(2, 5));
        let received = stream.next().await.unwrap().unwrap();
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

        let confirmation_stream = pin!(stream! {
            confirmation_stream_start.notified().await;
            yield 2;
        });
        let block1 = get_test_block(1, 3);
        let block2 = get_test_block(2, 5);
        let block3 = get_test_block(3, 3);
        let expected_unconfirmed_l2_blocks_before_confirmation =
            vec![block1.clone(), block2.clone(), block3.clone()];
        let final_expected_unconfirmed_l2_blocks = vec![block3.clone()];
        let l2_block_stream = pin!(stream! {
            yield block1;
            yield block2;
            yield block3;
            confirmation_stream_trigger.notify_one();
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: u64| async move { Ok(get_header(id, 0u64)) };
        let mut verifier = MockILastBatchVerifier::new();
        verifier
            .expect_verify()
            .return_once(|_| Box::pin(async { Ok(true) }));
        let stream = get_l2_head_stream(
            l2_block_stream,
            confirmation_stream,
            unconfirmed_l2_blocks.clone(),
            f,
            None,
            verifier,
        )
        .await;
        pin_mut!(stream);

        let received = stream.next().await.unwrap().unwrap();
        assert_eq!(received, get_header(1, 3));
        let received = stream.next().await.unwrap().unwrap();
        assert_eq!(received, get_header(2, 5));
        let received = stream.next().await.unwrap().unwrap();
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
        let confirmation_stream = pin!(stream! {
            confirmation_stream_start.notified().await;
            yield 1;
        });
        let block1 = get_test_block_with_txs(1, 3, txs);
        let block2 = get_test_block(2, 5);
        let expected_unconfirmed_l2_blocks_before_confirmation =
            vec![block1.clone(), block2.clone()];
        let final_expected_unconfirmed_l2_blocks = vec![block2.clone()];
        let l2_block_stream = pin!(stream! {
            yield block1;
            yield block2;
            confirmation_stream_trigger.notify_one();
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: u64| async move { Ok(get_header(id, 0u64)) };
        let mut verifier = MockILastBatchVerifier::new();
        verifier
            .expect_verify()
            .return_once(|_| Box::pin(async { Ok(true) }));
        let stream = get_l2_head_stream(
            l2_block_stream,
            confirmation_stream,
            unconfirmed_l2_blocks.clone(),
            f,
            None,
            verifier,
        )
        .await;
        pin_mut!(stream);

        let received = stream.next().await.unwrap().unwrap();
        assert_eq!(received, get_header(1, 3));
        let received = stream.next().await.unwrap().unwrap();
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
        let confirmation_stream = pin!(stream! {
            confirmation_stream_start.notified().await;
            yield 1;
        });
        let block1 = get_test_block_with_txs(1, 3, txs);
        let block2 = get_test_block(2, 5);
        let expected_unconfirmed_l2_blocks_before_confirmation =
            vec![block1.clone(), block2.clone()];
        let final_expected_unconfirmed_l2_blocks = vec![];
        let l2_block_stream = pin!(stream! {
            yield block1;
            yield block2;
            confirmation_stream_trigger.notify_one();
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: u64| async move { Ok(get_header(id, 0u64)) };
        let mut verifier = MockILastBatchVerifier::new();
        verifier
            .expect_verify()
            .return_once(|_| Box::pin(async { Ok(false) }));
        let stream = get_l2_head_stream(
            l2_block_stream,
            confirmation_stream,
            unconfirmed_l2_blocks.clone(),
            f,
            None,
            verifier,
        )
        .await;
        pin_mut!(stream);

        let received = stream.next().await.unwrap().unwrap();
        assert_eq!(received, get_header(1, 3));
        let received = stream.next().await.unwrap().unwrap();
        assert_eq!(received, get_header(2, 5));
        assert_eq!(
            *unconfirmed_l2_blocks.read().await,
            expected_unconfirmed_l2_blocks_before_confirmation
        );

        let received = stream.next().await.unwrap().unwrap();
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
        let confirmation_stream = pin!(stream! {
            confirmation_stream_start.notified().await;
            yield 1;
        });
        let block1 = get_test_block_with_txs(1, 3, txs);
        let block2 = get_test_block(2, 5);
        let expected_unconfirmed_l2_blocks_before_confirmation =
            vec![block1.clone(), block2.clone()];
        let final_expected_unconfirmed_l2_blocks = vec![block2.clone()];
        let l2_block_stream = pin!(stream! {
            yield block1;
            yield block2;
            confirmation_stream_trigger.notify_one();
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: u64| async move { Ok(get_header(id, 0u64)) };
        let mut verifier = MockILastBatchVerifier::new();
        verifier
            .expect_verify()
            .return_once(|_| Box::pin(async { Ok(true) }));
        let stream = get_l2_head_stream(
            l2_block_stream,
            confirmation_stream,
            unconfirmed_l2_blocks.clone(),
            f,
            None,
            verifier,
        )
        .await;
        pin_mut!(stream);

        let received = stream.next().await.unwrap().unwrap();
        assert_eq!(received, get_header(1, 3));
        let received = stream.next().await.unwrap().unwrap();
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
        let confirmation_stream = pin!(stream! {
            confirmation_stream_start.notified().await;
            yield 2;
        });
        let block1 = get_test_block_with_txs(1, 3, txs1);
        let block2 = get_test_block_with_txs(2, 5, txs2);
        let block3 = get_test_block(3, 3);
        let expected_unconfirmed_l2_blocks_before_confirmation =
            vec![block1.clone(), block2.clone(), block3.clone()];
        let final_expected_unconfirmed_l2_blocks = vec![block3.clone()];
        let l2_block_stream = pin!(stream! {
            yield block1;
            yield block2;
            yield block3;
            confirmation_stream_trigger.notify_one();
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: u64| async move { Ok(get_header(id, 0u64)) };
        let mut verifier = MockILastBatchVerifier::new();
        verifier
            .expect_verify()
            .return_once(|_| Box::pin(async { Ok(true) }));
        let stream = get_l2_head_stream(
            l2_block_stream,
            confirmation_stream,
            unconfirmed_l2_blocks.clone(),
            f,
            None,
            verifier,
        )
        .await;
        pin_mut!(stream);

        let received = stream.next().await.unwrap().unwrap();
        assert_eq!(received, get_header(1, 3));
        let received = stream.next().await.unwrap().unwrap();
        assert_eq!(received, get_header(2, 5));
        let received = stream.next().await.unwrap().unwrap();
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
}
