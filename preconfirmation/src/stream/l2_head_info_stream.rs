use std::{sync::Arc, time::Duration};

use alloy_consensus::Header;
use alloy_rpc_types_eth::Block;
use async_stream::stream;
use futures::{Stream, future::BoxFuture, pin_mut};
use tokio::sync::RwLock;
use tokio_stream::StreamExt;
use tracing::{info, warn};

use crate::{
    taiko::{contracts::TaikoInboxInstance, taiko_l1_client::TaikoL1ClientError},
    util::log_error,
    verification::ILastBatchVerifier,
};

enum StreamData {
    Unconfirmed(Box<Block>),
    Confirmed(u64),
}

async fn update_unconfirmed_blocks(block: Block, unconfirmed_l2_blocks: &Arc<RwLock<Vec<Block>>>) {
    if block.transactions.is_empty() {
        warn!("Ignoring empty block {}", block.header.number);
    }
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

pub fn get_id_stream(
    client_stream: impl Stream<Item = u64>,
    stream: impl Stream<Item = u64>,
) -> impl Stream<Item = u64> {
    let mut last_id = 0u64;
    stream.merge(client_stream).filter(move |id: &u64| {
        if id != &last_id {
            last_id = *id;
            true
        } else {
            false
        }
    })
}

pub fn get_confirmed_id_polling_stream(
    taiko_inbox: TaikoInboxInstance,
    polling_duration: Duration,
) -> impl Stream<Item = Result<u64, crate::verification::TaikoInboxError>> {
    stream! {
        let mut confirmed_id = 0u64;
        loop {
            let stats2 = taiko_inbox.getStats2().call().await?;
            if stats2.numBatches > 0 {
                let batch = taiko_inbox.getBatch(stats2.numBatches - 1).call().await?;
                if batch.lastBlockId > confirmed_id {
                    confirmed_id = batch.lastBlockId;
                    yield Ok(confirmed_id);
                }
            }
            tokio::time::sleep(polling_duration).await;
        }
    }
}

pub async fn get_l2_head_stream<F, FnFut, Verifier>(
    l2_block_stream: impl Stream<Item = Block>,
    confirmed_last_block_id_stream: impl Stream<Item = u64>,
    unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>>,
    get_block: F,
    initial_confirmed_block_id: Option<u64>,
    verifier: Verifier,
) -> impl Stream<Item = Result<Header, crate::taiko::taiko_l1_client::TaikoL1ClientError>>
where
    FnFut: Future<Output = Result<Block, reqwest::Error>> + Send + 'static,
    F: Fn(Option<u64>) -> FnFut,
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
                    info!("Received l2 block: {} #txs: {}", block.header.number, block.transactions.len());
                    if !block.transactions.is_empty() {
                    let header = block.header.inner.clone();
                    let should_yield = if let Some(last_confirmed_block_id) = *last_confirmed_block_id.read().await {
                        info!("last confirmed block: {last_confirmed_block_id}");
                        if block.header.number >= last_confirmed_block_id {
                            if block.header.number > last_confirmed_block_id {
                                update_unconfirmed_blocks(*block, &unconfirmed_l2_blocks).await;
                            }
                            true
                        } else {
                            false
                        }
                    } else {
                        update_unconfirmed_blocks(*block, &unconfirmed_l2_blocks).await;
                        true
                    };

                    if should_yield {
                        let current_block_number = header.number;
                        yield Ok(header);

                        let current_max_header_number = unconfirmed_l2_blocks.read().await.iter().map(|block| block.header.number).max().unwrap_or(current_block_number);
                        if current_block_number < current_max_header_number {
                            let mut latest_block_number = current_block_number;
                            // update other headers
                            if let Some(latest_block) = log_error(get_block(None).await, "Failed to get latest block") {
                                if latest_block.header.number > current_block_number {
                                    latest_block_number = latest_block.header.number;
                                    for id in (current_block_number + 1)..latest_block.header.number {
                                        if let Some(block) = log_error(get_block(Some(id)).await, &format!("Failed to update block {id}")) {
                                            let header = block.header.inner.clone();
                                            info!("Updated block {} with {} txs.", header.number, block.transactions.len());
                                            update_unconfirmed_blocks(block, &unconfirmed_l2_blocks).await;
                                            yield Ok(header);
                                        }
                                    }
                                    let header = latest_block.header.inner.clone();
                                    update_unconfirmed_blocks(latest_block, &unconfirmed_l2_blocks).await;
                                    yield Ok(header);
                                }
                            }
                            unconfirmed_l2_blocks.write().await.retain(|block| block.header.number <= latest_block_number);
                        }
                    }
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
                            log_error(get_block(Some(confirmed_block_id)).await, "Failed to query last block").map(|block| block.header.into())
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
    E: std::convert::From<TaikoL1ClientError>,
    T: Fn(Header, Value) -> BoxFuture<'a, Result<(), E>>,
>(
    stream: impl Stream<Item = Result<Header, TaikoL1ClientError>>,
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

    use async_stream::stream;

    use std::{pin::pin, sync::Arc};
    use tokio::sync::Notify;

    use crate::{
        test_util::{get_block, get_block_with_txs, get_dummy_signed_transaction, get_header},
        verification::MockILastBatchVerifier,
    };

    use super::*;

    #[tokio::test]
    async fn header_info_stream_returns_info_from_l2_blocks_if_no_confirmed_blocks_are_yet_available()
     {
        let confirmation_stream_start = Arc::new(Notify::new());

        let confirmation_stream = pin!(stream! {
            confirmation_stream_start.notified().await;
            yield 1;
        });
        let block1 = get_block(1, 3);
        let block2 = get_block(2, 5);
        let expected_unconfirmed_l2_blocks = vec![block1.clone(), block2.clone()];
        let l2_block_stream = pin!(stream! {
            yield block1;
            yield block2;
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: Option<u64>| async move { Ok(get_block(id.unwrap_or_default(), 0u64)) };
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
        let block1 = get_block(1, 3);
        let block2 = get_block(1, 5);
        let expected_unconfirmed_l2_blocks = vec![block2.clone()];
        let l2_block_stream = pin!(stream! {
            yield block1;
            yield block2;
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: Option<u64>| async move { Ok(get_block(id.unwrap_or_default(), 0u64)) };
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
    async fn header_info_stream_replaces_updated_l2_blocks_in_unconfirmed_blocks_and_all_blocks_after()
     {
        let confirmation_stream_start = Arc::new(Notify::new());

        let confirmation_stream = pin!(stream! {
            confirmation_stream_start.notified().await;
            yield 1;
        });
        let block1 = get_block(1, 3);
        let block2 = get_block(2, 4);
        let block3 = get_block(1, 5);
        let expected_unconfirmed_l2_blocks = vec![block3.clone(), get_block(2, 0)];
        let l2_block_stream = pin!(stream! {
            yield block1;
            yield block2;
            yield block3;
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: Option<u64>| async move {
            if let Some(id) = id {
                Ok(get_block(id, 0))
            } else {
                Ok(get_block(2, 0))
            }
        };
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
        assert_eq!(received, get_header(2, 4));
        assert_eq!(
            *unconfirmed_l2_blocks.read().await,
            vec![get_block(1, 3), get_block(2, 4)],
        );
        let received = stream.next().await.unwrap().unwrap();
        assert_eq!(received, get_header(1, 5));
        let received = stream.next().await.unwrap().unwrap();
        assert_eq!(received, get_header(2, 0));
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
        let block1 = get_block(1, 3);
        let block2 = get_block(1, 5);
        let expected_unconfirmed_l2_blocks = vec![];
        let l2_block_stream = pin!(stream! {
            yield block1;
            yield block2;
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: Option<u64>| async move { Ok(get_block(id.unwrap_or_default(), 0u64)) };
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
        let block3 = get_block(3, 3);
        let expected_unconfirmed_l2_blocks = vec![block3.clone()];
        let l2_block_stream = pin!(stream! {
            l2_block_stream_start.notified().await;
            yield get_block(1, 3);
            yield get_block(2, 5);
            yield block3;
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: Option<u64>| async move { Ok(get_block(id.unwrap_or_default(), 0u64)) };
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
        let block1 = get_block(1, 3);
        let block2 = get_block(2, 5);
        let block3 = get_block(3, 3);
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
        let f = |id: Option<u64>| async move { Ok(get_block(id.unwrap_or_default(), 0u64)) };
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

        let txs = vec![get_dummy_signed_transaction(0)];
        let confirmation_stream = pin!(stream! {
            confirmation_stream_start.notified().await;
            yield 1;
        });
        let block1 = get_block_with_txs(1, 3, txs);
        let block2 = get_block(2, 5);
        let expected_unconfirmed_l2_blocks_before_confirmation =
            vec![block1.clone(), block2.clone()];
        let final_expected_unconfirmed_l2_blocks = vec![block2.clone()];
        let l2_block_stream = pin!(stream! {
            yield block1;
            yield block2;
            confirmation_stream_trigger.notify_one();
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: Option<u64>| async move { Ok(get_block(id.unwrap_or_default(), 0u64)) };
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

        let txs = vec![get_dummy_signed_transaction(0)];
        let confirmation_stream = pin!(stream! {
            confirmation_stream_start.notified().await;
            yield 1;
        });
        let block1 = get_block_with_txs(1, 3, txs);
        let block2 = get_block(2, 5);
        let expected_unconfirmed_l2_blocks_before_confirmation =
            vec![block1.clone(), block2.clone()];
        let final_expected_unconfirmed_l2_blocks = vec![];
        let l2_block_stream = pin!(stream! {
            yield block1;
            yield block2;
            confirmation_stream_trigger.notify_one();
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: Option<u64>| async move { Ok(get_block(id.unwrap_or_default(), 0u64)) };
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
            get_dummy_signed_transaction(0),
            get_dummy_signed_transaction(1),
        ];
        let confirmation_stream = pin!(stream! {
            confirmation_stream_start.notified().await;
            yield 1;
        });
        let block1 = get_block_with_txs(1, 3, txs);
        let block2 = get_block(2, 5);
        let expected_unconfirmed_l2_blocks_before_confirmation =
            vec![block1.clone(), block2.clone()];
        let final_expected_unconfirmed_l2_blocks = vec![block2.clone()];
        let l2_block_stream = pin!(stream! {
            yield block1;
            yield block2;
            confirmation_stream_trigger.notify_one();
        });
        let unconfirmed_l2_blocks: Arc<RwLock<Vec<Block>>> = Arc::new(RwLock::new(vec![]));
        let f = |id: Option<u64>| async move { Ok(get_block(id.unwrap_or_default(), 0u64)) };
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
            get_dummy_signed_transaction(0),
            get_dummy_signed_transaction(1),
        ];
        let txs2 = vec![
            get_dummy_signed_transaction(2),
            get_dummy_signed_transaction(3),
        ];
        let confirmation_stream = pin!(stream! {
            confirmation_stream_start.notified().await;
            yield 2;
        });
        let block1 = get_block_with_txs(1, 3, txs1);
        let block2 = get_block_with_txs(2, 5, txs2);
        let block3 = get_block(3, 3);
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
        let f = |id: Option<u64>| async move { Ok(get_block(id.unwrap_or_default(), 0u64)) };
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
