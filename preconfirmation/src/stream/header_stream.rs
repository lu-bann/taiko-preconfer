use std::time::Duration;

use alloy::consensus::Header;
use alloy::primitives::B256;
use alloy::providers::Provider;
use alloy::rpc::types::eth::Block;
use async_stream::stream;
use futures::Stream;
use tokio_stream::StreamExt;

pub fn get_header_stream(
    client_stream: impl Stream<Item = Header>,
    stream: impl Stream<Item = Header>,
) -> impl Stream<Item = Header> {
    let mut last_header_number = -1_i128;
    let mut last_hash = B256::ZERO;
    stream.merge(client_stream).filter(move |header: &Header| {
        if header.number as i128 != last_header_number || header.hash_slow() != last_hash {
            last_header_number = header.number as i128;
            last_hash = header.hash_slow();
            true
        } else {
            false
        }
    })
}

pub fn get_block_stream(
    client_stream: impl Stream<Item = Block>,
    stream: impl Stream<Item = Block>,
) -> impl Stream<Item = Block> {
    let mut last_header_number = -1_i128;
    let mut last_hash = B256::ZERO;
    stream.merge(client_stream).filter(move |block: &Block| {
        if block.header.number as i128 != last_header_number
            || block.header.hash_slow() != last_hash
        {
            last_header_number = block.header.number as i128;
            last_hash = block.header.hash_slow();
            true
        } else {
            false
        }
    })
}

pub fn get_header_polling_stream<P: Provider>(
    provider: P,
    polling_duration: Duration,
) -> impl Stream<Item = Header> {
    stream! {
        let mut last_header_number = -1_i128;
        let mut last_hash = B256::ZERO;
        loop {
            if let Ok(Some(block)) = provider.get_block_by_number(alloy::eips::BlockNumberOrTag::Latest).await {
                let header = block.header.inner;
                if header.number as i128 != last_header_number || header.hash_slow() != last_hash {
                    last_header_number = header.number as i128;
                    last_hash = header.hash_slow();
                    yield header;
                }
            }
            tokio::time::sleep(polling_duration).await;
        }
    }
}

pub fn get_block_polling_stream<P: Provider>(
    provider: P,
    polling_duration: Duration,
) -> impl Stream<Item = Block> {
    stream! {
        let mut last_header_number = -1_i128;
        let mut last_hash = B256::ZERO;
        loop {
            #[allow(clippy::collapsible_if)]
            if let Ok(Some(block)) = provider.get_block_by_number(alloy::eips::BlockNumberOrTag::Latest).full().await {
                if block.header.number as i128 != last_header_number || block.header.hash_slow() != last_hash {
                    last_header_number = block.header.number as i128;
                    last_hash = block.header.hash_slow();
                    yield block;
                }
            }
            tokio::time::sleep(polling_duration).await;
        }
    }
}

#[cfg(test)]
mod tests {

    use async_stream::stream;

    use std::{pin::pin, sync::Arc, time::Duration};
    use tokio::sync::Notify;

    use crate::test_util::get_header;

    use super::*;

    #[tokio::test]
    async fn test_get_header_stream_can_receive_value_from_second_stream() {
        let number = 3;
        let timestamp = 5;

        let trigger_client_stream = Arc::new(Notify::new());
        let client_stream_start = trigger_client_stream.clone();

        let client_stream = pin!(stream! {
            client_stream_start.notified().await;
            yield get_header(number, timestamp);
        });
        let stream = pin!(stream! {
            yield get_header(number, timestamp);
            trigger_client_stream.notify_one();
        });
        let mut header_stream = get_header_stream(client_stream, stream);
        let received = header_stream.next().await;
        assert_eq!(received, Some(get_header(number, timestamp)));
    }

    #[tokio::test]
    async fn test_get_header_stream_can_receive_value_from_first_stream() {
        let number = 3;
        let timestamp = 5;

        let trigger_stream = Arc::new(Notify::new());
        let stream_start = trigger_stream.clone();

        let client_stream = pin!(stream! {
            yield get_header(number, timestamp);
            trigger_stream.notify_one();
        });
        let stream = pin!(stream! {
            stream_start.notified().await;
            yield get_header(number, timestamp);
        });
        let mut header_stream = get_header_stream(client_stream, stream);
        let received = header_stream.next().await;
        assert_eq!(received, Some(get_header(number, timestamp)));
    }

    #[tokio::test]
    async fn test_get_header_stream_does_not_receive_value_from_any_stream() {
        let number = 3;
        let timestamp = 5;

        let trigger_stream = Arc::new(Notify::new());
        let trigger_client_stream = Arc::new(Notify::new());

        let client_stream = pin!(stream! {
            trigger_client_stream.notified().await;
            yield get_header(number, timestamp);
        });
        let stream = pin!(stream! {
            trigger_stream.notified().await;
            yield get_header(number, timestamp);
        });
        let mut header_stream = get_header_stream(client_stream, stream);
        let timeout = Duration::from_millis(30);
        assert!(
            tokio::time::timeout(timeout, header_stream.next())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_get_header_stream_filters_duplicate_blocks() {
        let number = 3;
        let timestamp = 5;

        let trigger_stream = Arc::new(Notify::new());
        let stream_start = trigger_stream.clone();

        let client_stream = pin!(stream! {
            yield get_header(number, timestamp);
            trigger_stream.notify_one();
        });
        let stream = pin!(stream! {
            stream_start.notified().await;
            yield get_header(number, timestamp);
        });
        let mut header_stream = get_header_stream(client_stream, stream);
        let received = header_stream.next().await;
        assert_eq!(received, Some(get_header(number, timestamp)));
        let received = header_stream.next().await;
        assert_eq!(received, None);
    }

    #[tokio::test]
    async fn test_get_header_stream_creates_considers_duplicate_block_numbers_if_hashes_differ() {
        let timestamp = 5;
        let number = 3;
        let other_timestamp = 3;

        let trigger_client_stream = Arc::new(Notify::new());
        let client_stream_start = trigger_client_stream.clone();

        let trigger_stream = Arc::new(Notify::new());
        let stream_start = trigger_stream.clone();

        let client_stream = pin!(stream! {
            yield get_header(number, timestamp);
            trigger_stream.notify_one();
            client_stream_start.notified().await;
            yield get_header(number, other_timestamp);
        });
        let stream = pin!(stream! {
            stream_start.notified().await;
            yield get_header(number, other_timestamp);
            trigger_client_stream.notify_one();
        });
        let mut header_stream = get_header_stream(client_stream, stream);
        let received = header_stream.next().await.unwrap();
        assert_eq!(received, get_header(number, timestamp));
        let received = header_stream.next().await.unwrap();
        assert_eq!(received, get_header(number, other_timestamp));
        assert_eq!(header_stream.next().await, None);
    }
}
