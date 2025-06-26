use std::time::Duration;

use alloy_consensus::Header;
use alloy_primitives::B256;
use alloy_rpc_client::RpcClient;
use alloy_rpc_types_eth::Block;
use async_stream::stream;
use futures::{Stream, future::BoxFuture, pin_mut};
use tokio_stream::StreamExt;

use crate::client::{get_latest_block, get_latest_header};

pub fn get_header_stream(
    client_stream: impl Stream<Item = Header>,
    stream: impl Stream<Item = Header>,
) -> impl Stream<Item = Header> {
    let mut last_header_number = -1_i128;
    let mut last_hash = B256::ZERO;
    stream.merge(client_stream).filter(move |header: &Header| {
        if header.number as i128 != last_header_number || header.mix_hash != last_hash {
            last_header_number = header.number as i128;
            last_hash = header.mix_hash;
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
        if block.header.number as i128 != last_header_number || block.header.mix_hash != last_hash {
            last_header_number = block.header.number as i128;
            last_hash = block.header.mix_hash;
            true
        } else {
            false
        }
    })
}

pub fn get_header_polling_stream(
    client: RpcClient,
    polling_duration: Duration,
) -> impl Stream<Item = Header> {
    stream! {
        let mut last_header_number = -1_i128;
        let mut last_hash = B256::ZERO;
        loop {
            if let Ok(header) =  get_latest_header(&client).await {
                if header.number as i128 != last_header_number || header.mix_hash != last_hash {
                    last_header_number = header.number as i128;
                    last_hash = header.mix_hash;
                    yield header;
                }
            }
            tokio::time::sleep(polling_duration).await;
        }
    }
}

pub fn get_block_polling_stream(
    client: RpcClient,
    polling_duration: Duration,
    full_tx: bool,
) -> impl Stream<Item = Block> {
    stream! {
        let mut last_header_number = -1_i128;
        let mut last_hash = B256::ZERO;
        loop {
            if let Ok(block) = get_latest_block(&client, full_tx).await {
                if block.header.number as i128 != last_header_number || block.header.mix_hash != last_hash {
                    last_header_number = block.header.number as i128;
                    last_hash = block.header.mix_hash;
                    yield block;
                }
            }
            tokio::time::sleep(polling_duration).await;
        }
    }
}

pub async fn stream_headers<
    'a,
    Value: Clone,
    E,
    T: Fn(Header, Value) -> BoxFuture<'a, Result<(), E>>,
>(
    stream: impl Stream<Item = Header>,
    f: T,
    current: Value,
) -> Result<(), E> {
    pin_mut!(stream);
    while let Some(header) = stream.next().await {
        f(header, current.clone()).await?;
    }
    Ok(())
}

pub fn to_boxed<'a, Value, F, FnFut, Res>(
    header: Header,
    current: Value,
    f: F,
) -> BoxFuture<'a, Res>
where
    Value: Send + 'static,
    FnFut: Future<Output = Res> + Send + 'static,
    F: Fn(Header, Value) -> FnFut,
{
    Box::pin(f(header, current))
}

#[cfg(test)]
mod tests {

    use async_stream::stream;

    use std::{pin::pin, sync::Arc, time::Duration};
    use tokio::sync::Notify;

    use crate::test_util::{get_header, get_header_with_mixhash};

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
        let timestamp_1 = 5;
        let timestamp_2 = 7;

        let trigger_stream = Arc::new(Notify::new());
        let stream_start = trigger_stream.clone();

        let client_stream = pin!(stream! {
            yield get_header(number, timestamp_1);
            trigger_stream.notify_one();
        });
        let stream = pin!(stream! {
            stream_start.notified().await;
            yield get_header(number, timestamp_2);
        });
        let mut header_stream = get_header_stream(client_stream, stream);
        let received = header_stream.next().await;
        assert_eq!(received, Some(get_header(number, timestamp_1)));
        let received = header_stream.next().await;
        assert_eq!(received, None);
    }

    #[tokio::test]
    async fn test_get_header_stream_creates_considers_duplicate_block_numbers_if_hashes_differ() {
        let timestamp = 5;
        let number = 3;
        let mixhash = B256::ZERO;
        let other_mixhash = B256::left_padding_from(&[1]);

        let trigger_client_stream = Arc::new(Notify::new());
        let client_stream_start = trigger_client_stream.clone();

        let trigger_stream = Arc::new(Notify::new());
        let stream_start = trigger_stream.clone();

        let client_stream = pin!(stream! {
            yield get_header_with_mixhash(number, timestamp, mixhash);
            trigger_stream.notify_one();
            client_stream_start.notified().await;
            yield get_header_with_mixhash(number, timestamp, other_mixhash);
        });
        let stream = pin!(stream! {
            stream_start.notified().await;
            yield get_header_with_mixhash(number, timestamp, mixhash);
            trigger_client_stream.notify_one();
        });
        let mut header_stream = get_header_stream(client_stream, stream);
        let received = header_stream.next().await.unwrap();
        assert_eq!(
            received,
            get_header_with_mixhash(number, timestamp, mixhash)
        );
        let received = header_stream.next().await.unwrap();
        assert_eq!(
            received,
            get_header_with_mixhash(number, timestamp, other_mixhash)
        );
        assert_eq!(header_stream.next().await, None);
    }
}
