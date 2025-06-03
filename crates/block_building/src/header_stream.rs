use std::time::Duration;

use alloy_consensus::Header;
use async_stream::stream;
use futures::{Stream, future::BoxFuture, pin_mut};
use tokio_stream::StreamExt;

use crate::http_client::{HttpClient, get_header};

pub fn get_header_stream(
    client_stream: impl Stream<Item = Header>,
    stream: impl Stream<Item = Header>,
) -> impl Stream<Item = Header> {
    let mut last_header_number = 0u64;
    stream.merge(client_stream).filter(move |header: &Header| {
        if header.number > last_header_number {
            last_header_number = header.number;
            true
        } else {
            false
        }
    })
}

pub fn get_polling_stream<Client: HttpClient>(
    client: Client,
    polling_duration: Duration,
) -> impl Stream<Item = Header> {
    stream! {
        let mut last_header_number = 0u64;
        loop {
            if let Ok(header) = get_header(&client, alloy_eips::BlockNumberOrTag::Latest).await {
                if header.number > last_header_number {
                    last_header_number = header.number;
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

#[cfg(test)]
mod tests {
    use std::{pin::pin, time::Duration};

    use async_stream::stream;

    use crate::test_util::get_header;

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
    async fn test_get_header_stream_creates_increasing_stream() {
        let timestamp = 5;
        let header_delay = Duration::from_millis(10);
        let provider_delay = Duration::from_secs(1);
        let first_number = 3;
        let second_number = 4;
        let third_number = 5;
        let fourth_number = 8;
        let client_stream = pin!(stream! {
            let number = first_number;
            yield get_header(number, timestamp);
            tokio::time::sleep(header_delay).await;
            let number = first_number - 1;
            yield get_header(number, timestamp);
            tokio::time::sleep(header_delay).await;
            let number = third_number;
            yield get_header(number, timestamp);
            tokio::time::sleep(provider_delay).await;
        });
        let stream = pin!(stream! {
            tokio::time::sleep(header_delay/2).await;
            let number = second_number;
            yield get_header(number, timestamp);
            tokio::time::sleep(header_delay).await;
            let number = first_number;
            yield get_header(number, timestamp);
            tokio::time::sleep(header_delay).await;
            let number = fourth_number;
            yield get_header(number, timestamp);
            tokio::time::sleep(provider_delay).await;
        });
        let mut header_stream = get_header_stream(client_stream, stream);
        let timeout = Duration::from_millis(30);
        let received = header_stream.next().await.unwrap();
        assert_eq!(received, get_header(first_number, timestamp));
        let received = header_stream.next().await.unwrap();
        assert_eq!(received, get_header(second_number, timestamp));
        let received = header_stream.next().await.unwrap();
        assert_eq!(received, get_header(third_number, timestamp));
        let received = header_stream.next().await.unwrap();
        assert_eq!(received, get_header(fourth_number, timestamp));
        assert!(
            tokio::time::timeout(timeout, header_stream.next())
                .await
                .is_err()
        );
    }
}
