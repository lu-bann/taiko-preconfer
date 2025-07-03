use std::num::TryFromIntError;

use futures::{StreamExt, stream::Stream};
use tokio::time::{Duration, Instant, interval_at};
use tokio_stream::wrappers::IntervalStream;

use crate::{
    slot::{Slot, SubSlot},
    time_provider::ITimeProvider,
};

pub fn get_slot_stream(
    start: Instant,
    next_slot_count: u64,
    slot_time: Duration,
    slots_per_epoch: u64,
) -> Result<impl Stream<Item = Slot>, TryFromIntError> {
    let mut interval = interval_at(start, slot_time);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut next_slot_count = next_slot_count;

    Ok(IntervalStream::new(interval).map(move |_| {
        let slot_count = next_slot_count;
        next_slot_count += 1;

        Slot::new(slot_count / slots_per_epoch, slot_count % slots_per_epoch)
    }))
}

pub fn get_subslot_stream(
    stream: impl Stream<Item = Slot>,
    subslots_per_slot: u64,
) -> impl Stream<Item = SubSlot> {
    stream.map(move |slot| {
        SubSlot::new(
            Slot::new(slot.epoch, slot.slot / subslots_per_slot),
            slot.slot,
        )
    })
}

pub fn get_next_slot_start<Provider: ITimeProvider>(
    slot_time: &Duration,
    provider: &Provider,
) -> Result<Instant, TryFromIntError> {
    let duration_now = Duration::from_millis(provider.timestamp_in_ms());
    let in_current_slot_ms: Duration =
        Duration::from_millis((duration_now.as_millis() % slot_time.as_millis()).try_into()?);
    let remaining = if in_current_slot_ms.is_zero() {
        Duration::ZERO
    } else {
        *slot_time - in_current_slot_ms
    };
    Ok(Instant::now() + remaining)
}

#[cfg(test)]
mod tests {
    use crate::{slot::SubSlot, time_provider::MockITimeProvider};

    use super::*;
    use futures::pin_mut;

    #[tokio::test(start_paused = true)]
    async fn if_at_genesis_then_stream_yields_slot_0_at_epoch_0() {
        let slot_time = Duration::from_millis(100);
        let slots_per_epoch = 10;
        let start = Instant::now();
        let next_slot_count = 0;
        let stream = get_slot_stream(start, next_slot_count, slot_time, slots_per_epoch).unwrap();
        pin_mut!(stream);
        let value = stream.next().await;
        assert!(value.is_some());
        assert_eq!(value.unwrap(), Slot::new(0, 0));
    }

    #[tokio::test(start_paused = true)]
    async fn if_started_half_slot_time_after_genesis_then_stream_yields_next_slot_1_at_epoch_0() {
        let slot_time = Duration::from_millis(100);
        let slots_per_epoch = 10;
        let start = Instant::now();
        tokio::time::advance(slot_time / 2).await;
        let next_slot_count = 1;
        let stream = get_slot_stream(start, next_slot_count, slot_time, slots_per_epoch).unwrap();
        pin_mut!(stream);
        let value = stream.next().await;
        assert!(value.is_some());
        assert_eq!(value.unwrap(), Slot::new(0, 1));
    }

    #[tokio::test(start_paused = true)]
    async fn if_started_one_slot_time_after_genesis_then_stream_yields_slot_1_at_epoch_0() {
        let slot_time = Duration::from_millis(100);
        let slots_per_epoch = 10;
        let start = Instant::now();
        tokio::time::advance(slot_time).await;
        let next_slot_count = 1;
        let stream = get_slot_stream(start, next_slot_count, slot_time, slots_per_epoch).unwrap();
        pin_mut!(stream);
        let value = stream.next().await;
        assert!(value.is_some());
        assert_eq!(value.unwrap(), Slot::new(0, 1));
    }

    #[tokio::test(start_paused = true)]
    async fn if_started_ten_slot_times_after_genesis_then_stream_yields_slot_0_at_epoch_1() {
        let slot_time = Duration::from_millis(100);
        let slots_per_epoch = 10;
        let start = Instant::now();
        tokio::time::advance(10 * slot_time).await;
        let next_slot_count = 10;
        let stream = get_slot_stream(start, next_slot_count, slot_time, slots_per_epoch).unwrap();
        pin_mut!(stream);
        let value = stream.next().await;
        assert!(value.is_some());
        assert_eq!(value.unwrap(), Slot::new(1, 0));
    }

    #[tokio::test(start_paused = true)]
    async fn if_at_genesis_then_sub_stream_yields_subslot_0_slot_0_at_epoch_0() {
        let slot_time = Duration::from_millis(100);
        let slots_per_epoch = 10;
        let subslots_per_slot = 2;
        let start = Instant::now();
        let next_slot_count = 0;
        let slot_stream =
            get_slot_stream(start, next_slot_count, slot_time, slots_per_epoch).unwrap();
        let stream = get_subslot_stream(slot_stream, subslots_per_slot);
        pin_mut!(stream);
        let value = stream.next().await;
        assert!(value.is_some());
        assert_eq!(value.unwrap(), SubSlot::new(Slot::new(0, 0), 0));
    }

    #[tokio::test(start_paused = true)]
    async fn if_started_half_slot_time_after_genesis_then_subslot_stream_yields_next_slot_1_at_epoch_0()
     {
        let slot_time = Duration::from_millis(100);
        let slots_per_epoch = 10;
        let subslots_per_slot = 2;
        let start = Instant::now();
        tokio::time::advance(slot_time / 2).await;
        let next_slot_count = 1;
        let slot_stream =
            get_slot_stream(start, next_slot_count, slot_time, slots_per_epoch).unwrap();
        let stream = get_subslot_stream(slot_stream, subslots_per_slot);
        pin_mut!(stream);
        let value = stream.next().await;
        assert!(value.is_some());
        assert_eq!(value.unwrap(), SubSlot::new(Slot::new(0, 0), 1));
    }

    #[tokio::test(start_paused = true)]
    async fn if_started_one_slot_time_after_genesis_then_subslot_stream_yields_next_subslot_1_slot_0_at_epoch_0()
     {
        let slot_time = Duration::from_millis(100);
        let slots_per_epoch = 10;
        let subslots_per_slot = 2;
        let start = Instant::now();
        tokio::time::advance(slot_time).await;
        let next_slot_count = 1;
        let slot_stream =
            get_slot_stream(start, next_slot_count, slot_time, slots_per_epoch).unwrap();
        let stream = get_subslot_stream(slot_stream, subslots_per_slot);
        pin_mut!(stream);
        let value = stream.next().await;
        assert!(value.is_some());
        assert_eq!(value.unwrap(), SubSlot::new(Slot::new(0, 0), 1));
    }

    #[tokio::test(start_paused = true)]
    async fn if_started_two_slot_times_after_genesis_then_subslot_stream_yields_next_subslot_2_slot_1_at_epoch_0()
     {
        let slot_time = Duration::from_millis(100);
        let slots_per_epoch = 10;
        let subslots_per_slot = 2;
        let start = Instant::now();
        tokio::time::advance(2 * slot_time).await;
        let next_slot_count = 2;
        let slot_stream =
            get_slot_stream(start, next_slot_count, slot_time, slots_per_epoch).unwrap();
        let stream = get_subslot_stream(slot_stream, subslots_per_slot);
        pin_mut!(stream);
        let value = stream.next().await;
        assert!(value.is_some());
        assert_eq!(value.unwrap(), SubSlot::new(Slot::new(0, 1), 2));
    }

    #[tokio::test(start_paused = true)]
    async fn if_started_ten_slot_times_after_genesis_then_subslot_stream_yields_subslot_0_slot_0_at_epoch_1()
     {
        let slot_time = Duration::from_millis(100);
        let slots_per_epoch = 10;
        let subslots_per_slot = 2;
        let start = Instant::now();
        tokio::time::advance(10 * slot_time).await;
        let next_slot_count = 10;
        let slot_stream =
            get_slot_stream(start, next_slot_count, slot_time, slots_per_epoch).unwrap();
        let stream = get_subslot_stream(slot_stream, subslots_per_slot);
        pin_mut!(stream);
        let value = stream.next().await;
        assert!(value.is_some());
        assert_eq!(value.unwrap(), SubSlot::new(Slot::new(1, 0), 0));
    }

    #[tokio::test(start_paused = true)]
    async fn test_get_next_slot_start_provides_time_until_next_block_start() {
        let slot_time = Duration::from_secs(10);
        let now = Instant::now();

        let timestamp: u64 = 13000;
        let mut time_provider = MockITimeProvider::new();
        time_provider
            .expect_timestamp_in_ms()
            .return_const(timestamp);

        let next_slot_start = get_next_slot_start(&slot_time, &time_provider).unwrap();
        assert_eq!(next_slot_start, now + Duration::from_secs(7));
    }

    #[tokio::test(start_paused = true)]
    async fn test_get_next_slot_start_is_now_when_at_slot_start() {
        let slot_time = Duration::from_secs(10);
        let now = Instant::now();

        let timestamp: u64 = 20000;
        let mut time_provider = MockITimeProvider::new();
        time_provider
            .expect_timestamp_in_ms()
            .return_const(timestamp);

        let next_slot_start = get_next_slot_start(&slot_time, &time_provider).unwrap();
        assert_eq!(next_slot_start, now + Duration::from_secs(0));
    }
}
