use std::time::Duration;

#[cfg_attr(test, mockall::automock)]
pub trait SequencingMonitor {
    fn end_received(&self) -> impl Future<Output = ()>;
    fn in_sync_with_l2_head(&self) -> impl Future<Output = ()>;
}

pub struct DummySequencingMonitor {}

impl SequencingMonitor for DummySequencingMonitor {
    async fn end_received(&self) {}

    async fn in_sync_with_l2_head(&self) {}
}

pub async fn wait_for_end_of_sequencing<Monitor: SequencingMonitor>(
    timeout: Duration,
    monitor: &Monitor,
) {
    let _ = tokio::time::timeout(timeout, monitor.end_received()).await;
}

pub async fn end_of_handover_start_buffer<Monitor: SequencingMonitor>(
    handover_timeout: Duration,
    monitor: &Monitor,
) {
    tokio::join!(
        wait_for_end_of_sequencing(handover_timeout, monitor),
        monitor.in_sync_with_l2_head(),
    );
}

#[cfg(test)]
mod tests {
    use std::task::{Context, Poll};

    use tokio::pin;

    use super::*;

    #[tokio::test(start_paused = true)]
    async fn wait_for_end_of_sequencing_ends_when_end_of_sequencing_is_received() {
        let timeout = Duration::from_millis(100);
        let mut monitor = MockSequencingMonitor::new();
        monitor
            .expect_end_received()
            .return_once(|| Box::pin(async {}));
        let result_future = wait_for_end_of_sequencing(timeout, &monitor);
        pin!(result_future);
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        tokio::time::advance(timeout / 2).await;
        assert_eq!(result_future.poll(&mut cx), Poll::Ready(()));
    }

    #[tokio::test(start_paused = true)]
    async fn wait_for_end_of_sequencing_ends_when_timeout_is_passed() {
        let timeout = Duration::from_millis(100);
        let monitor_delay = 3 * timeout;
        let mut monitor = MockSequencingMonitor::new();
        monitor.expect_end_received().return_once(move || {
            Box::pin(async move {
                tokio::time::sleep(monitor_delay).await;
            })
        });
        let result_future = wait_for_end_of_sequencing(timeout, &monitor);
        pin!(result_future);
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        let _ = result_future.as_mut().poll(&mut cx);
        tokio::time::advance(2 * timeout).await;
        assert_eq!(result_future.poll(&mut cx), Poll::Ready(()));
    }

    #[tokio::test(start_paused = true)]
    async fn wait_for_end_of_sequencing_is_pending_when_within_timeout_and_no_end_received() {
        let timeout = Duration::from_millis(100);
        let monitor_delay = 3 * timeout;
        let mut monitor = MockSequencingMonitor::new();
        monitor.expect_end_received().return_once(move || {
            Box::pin(async move {
                tokio::time::sleep(monitor_delay).await;
            })
        });
        let result_future = wait_for_end_of_sequencing(timeout, &monitor);
        pin!(result_future);
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        let _ = result_future.as_mut().poll(&mut cx);
        tokio::time::advance(timeout / 2).await;
        assert_eq!(result_future.poll(&mut cx), Poll::Pending);
    }

    #[tokio::test(start_paused = true)]
    async fn end_of_handover_start_buffer_ends_when_end_of_sequencing_is_receive_and_l2_head_is_in_sync()
     {
        let timeout = Duration::from_millis(100);
        let mut monitor = MockSequencingMonitor::new();
        monitor
            .expect_end_received()
            .return_once(|| Box::pin(async {}));
        monitor
            .expect_in_sync_with_l2_head()
            .return_once(|| Box::pin(async {}));
        let result_future = end_of_handover_start_buffer(timeout, &monitor);
        pin!(result_future);
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        tokio::time::advance(timeout / 2).await;
        assert_eq!(result_future.poll(&mut cx), Poll::Ready(()));
    }

    #[tokio::test(start_paused = true)]
    async fn end_of_handover_start_buffer_is_pending_when_end_of_sequencing_is_receive_and_l2_head_is_not_in_sync()
     {
        let timeout = Duration::from_millis(100);
        let monitor_delay = 3 * timeout;
        let mut monitor = MockSequencingMonitor::new();
        monitor
            .expect_end_received()
            .return_once(|| Box::pin(async {}));
        monitor.expect_in_sync_with_l2_head().return_once(move || {
            Box::pin(async move {
                tokio::time::sleep(monitor_delay).await;
            })
        });
        let result_future = end_of_handover_start_buffer(timeout, &monitor);
        pin!(result_future);
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        tokio::time::advance(timeout / 2).await;
        assert_eq!(result_future.poll(&mut cx), Poll::Pending);
    }

    #[tokio::test(start_paused = true)]
    async fn end_of_handover_start_buffer_ends_when_timeout_is_passed_and_is_in_sync_with_l2_head()
    {
        let timeout = Duration::from_millis(100);
        let monitor_delay = 3 * timeout;
        let mut monitor = MockSequencingMonitor::new();
        monitor.expect_end_received().return_once(move || {
            Box::pin(async move {
                tokio::time::sleep(monitor_delay).await;
            })
        });
        monitor
            .expect_in_sync_with_l2_head()
            .return_once(|| Box::pin(async {}));
        let result_future = end_of_handover_start_buffer(timeout, &monitor);
        pin!(result_future);
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        let _ = result_future.as_mut().poll(&mut cx);
        tokio::time::advance(2 * timeout).await;
        assert_eq!(result_future.poll(&mut cx), Poll::Ready(()));
    }

    #[tokio::test(start_paused = true)]
    async fn end_of_handover_start_buffer_is_pending_when_timeout_is_passed_and_is_not_in_sync_with_l2_head()
     {
        let timeout = Duration::from_millis(100);
        let monitor_delay = 3 * timeout;
        let mut monitor = MockSequencingMonitor::new();
        monitor.expect_end_received().return_once(move || {
            Box::pin(async move {
                tokio::time::sleep(monitor_delay).await;
            })
        });
        monitor.expect_in_sync_with_l2_head().return_once(move || {
            Box::pin(async move {
                tokio::time::sleep(monitor_delay).await;
            })
        });
        let result_future = end_of_handover_start_buffer(timeout, &monitor);
        pin!(result_future);
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        let _ = result_future.as_mut().poll(&mut cx);
        tokio::time::advance(2 * timeout).await;
        assert_eq!(result_future.poll(&mut cx), Poll::Pending);
    }
}
