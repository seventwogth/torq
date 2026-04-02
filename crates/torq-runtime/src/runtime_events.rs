use tokio::sync::mpsc;
use torq_core::TorEvent;

// The internal runtime pipeline is intentionally bounded so noisy log streams
// cannot grow memory without limit. Short bursts are buffered, and overload is
// handled explicitly in RuntimeEventSender::send.
pub(crate) const RUNTIME_EVENT_QUEUE_CAPACITY: usize = 1024;

#[derive(Clone, Debug)]
pub(crate) struct RuntimeEventSender {
    inner: mpsc::Sender<TorEvent>,
}

impl RuntimeEventSender {
    pub(crate) fn channel(capacity: usize) -> (Self, mpsc::Receiver<TorEvent>) {
        let (inner, rx) = mpsc::channel(capacity);
        (Self { inner }, rx)
    }

    pub(crate) async fn send(&self, event: TorEvent) {
        if Self::is_lossy(&event) {
            self.send_lossy(event).await;
            return;
        }

        self.send_reliably(event).await;
    }

    pub(crate) async fn send_all<I>(&self, events: I)
    where
        I: IntoIterator<Item = TorEvent>,
    {
        for event in events {
            self.send(event).await;
        }
    }

    fn is_lossy(event: &TorEvent) -> bool {
        matches!(event, TorEvent::LogLine(_))
    }

    async fn send_lossy(&self, event: TorEvent) {
        // Raw log lines are the only intentionally lossy events. If the queue is
        // full, we drop them rather than blocking state-bearing events behind a
        // long burst of log traffic.
        match self.inner.try_send(event) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(TorEvent::LogLine(_))) => {}
            Err(mpsc::error::TrySendError::Closed(_)) => {}
            Err(mpsc::error::TrySendError::Full(other)) => self.send_reliably(other).await,
        }
    }

    async fn send_reliably(&self, event: TorEvent) {
        // State-bearing and operator-visible events must not be dropped on
        // overload, so they wait for capacity instead of being discarded.
        let _ = self.inner.send(event).await;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::sleep;
    use torq_core::TorEvent;

    use super::RuntimeEventSender;

    #[tokio::test]
    async fn drops_log_line_when_queue_is_full() {
        let (sender, mut rx) = RuntimeEventSender::channel(1);

        sender.send(TorEvent::Started).await;
        sender
            .send(TorEvent::LogLine("very noisy log".to_string()))
            .await;

        assert_eq!(rx.recv().await, Some(TorEvent::Started));
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn critical_events_wait_for_capacity() {
        let (sender, mut rx) = RuntimeEventSender::channel(1);

        sender.send(TorEvent::Bootstrap(10)).await;

        let blocked_sender = sender.clone();
        let send_task = tokio::spawn(async move {
            blocked_sender.send(TorEvent::Started).await;
        });

        sleep(Duration::from_millis(10)).await;
        assert!(!send_task.is_finished());

        assert_eq!(rx.recv().await, Some(TorEvent::Bootstrap(10)));
        send_task.await.unwrap();
        assert_eq!(rx.recv().await, Some(TorEvent::Started));
    }
}
