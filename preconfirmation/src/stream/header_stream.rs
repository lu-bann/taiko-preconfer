use std::time::Duration;

use alloy_consensus::Header;
use alloy_primitives::B256;
use alloy_rpc_client::RpcClient;
use async_stream::stream;
use futures::{Stream, future::BoxFuture, pin_mut};
use tokio_stream::StreamExt;

use crate::client::get_latest_header;

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

pub fn get_polling_stream(
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
    use std::{pin::pin, time::Duration};

    use async_stream::stream;

    use crate::test_util::{get_header, get_header_with_mixhash};

    use super::*;

    #[tokio::test]
    async fn test_get_header_stream_can_receive_value_from_second_stream() {
        let number = 3;
        let timestamp = 5;
        let provider_delay = Duration::from_secs(1);
        let client_stream = pin!(stream! {
            tokio::time::sleep(provider_delay).await;
            yield get_header(number, timestamp);
        });
        let stream = pin!(stream! {
            yield get_header(number, timestamp);
        });
        let mut header_stream = get_header_stream(client_stream, stream);
        let received = header_stream.next().await;
        assert_eq!(received, Some(get_header(number, timestamp)));
    }

    #[tokio::test]
    async fn test_get_header_stream_can_receive_value_from_first_stream() {
        let number = 3;
        let timestamp = 5;
        let provider_delay = Duration::from_secs(1);
        let client_stream = pin!(stream! {
            yield get_header(number, timestamp);
        });
        let stream = pin!(stream! {
            tokio::time::sleep(provider_delay).await;
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
        let provider_delay = Duration::from_secs(1);
        let client_stream = pin!(stream! {
            tokio::time::sleep(provider_delay).await;
            yield get_header(number, timestamp);
        });
        let stream = pin!(stream! {
            tokio::time::sleep(provider_delay).await;
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
    async fn test_get_header_stream_filters_duplicate_block_numbers() {
        let number = 3;
        let timestamp_1 = 5;
        let timestamp_2 = 7;
        let provider_delay = Duration::from_secs(1);
        let client_stream = pin!(stream! {
            yield get_header(number, timestamp_1);
            tokio::time::sleep(provider_delay).await;
        });
        let stream = pin!(stream! {
            tokio::time::sleep(Duration::from_millis(1)).await;
            yield get_header(number, timestamp_2);
            tokio::time::sleep(provider_delay).await;
        });
        let mut header_stream = get_header_stream(client_stream, stream);
        let timeout = Duration::from_millis(30);
        let received = tokio::time::timeout(timeout, header_stream.next())
            .await
            .unwrap();
        assert_eq!(received, Some(get_header(number, timestamp_1)));
        assert!(
            tokio::time::timeout(timeout, header_stream.next())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_get_header_stream_creates_considers_duplicate_block_numbers_if_hashes_differ() {
        let timestamp = 5;
        let header_delay = Duration::from_millis(10);
        let number = 3;
        let mixhash = B256::ZERO;
        let other_mixhash = B256::left_padding_from(&[1]);
        let client_stream = pin!(stream! {
            yield get_header_with_mixhash(number, timestamp, mixhash);
            tokio::time::sleep(header_delay).await;
            yield get_header_with_mixhash(number, timestamp, other_mixhash);
        });
        let stream = pin!(stream! {
            tokio::time::sleep(header_delay/2).await;
            yield get_header_with_mixhash(number, timestamp, mixhash);
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
