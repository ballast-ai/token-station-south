//! Explicit host observation; payloads, credentials and socket addresses are never emitted.
use bytes::Bytes;
use std::{collections::VecDeque, net::SocketAddr, sync::Mutex};
use std::{
    convert::Infallible,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, Instant},
};
/// Optional synchronous host callbacks for buffered requests carrying an opaque request ID.
///
/// Callbacks must return promptly, must not block or panic, and must never use credentials or
/// personal data as IDs. Durations use this transport request's monotonic clock, not server time.
/// Missing or invalid IDs suppress observation without changing the request.
pub trait TransportObserver: Send + Sync {
    /// Reserved for the separately applied local-write patch; stock reqwest never calls this.
    fn local_write(&self, _id: &str, _stage: &'static str, _body_bytes: u64) {}

    /// Reports bounded tuple history, not proof of physical connection reuse.
    fn connection(
        &self,
        _id: &str,
        _elapsed: Duration,
        _fingerprint: u64,
        _reused: bool,
        _protocol: &str,
    ) {
    }
    /// Reports a transport stage; body delivery never implies upload or upstream receipt.
    fn event(&self, request_id: &str, stage: &'static str, elapsed: Duration);
}
type SocketPair = (SocketAddr, SocketAddr);
const MAX_CONNECTION_HISTORY: usize = 256;
const CONNECTION_HISTORY_TTL: Duration = Duration::from_mins(1);
#[derive(Default)]
struct History {
    entries: VecDeque<(SocketPair, u64, Instant, Instant)>,
    next_id: u64,
}
impl History {
    fn record(&mut self, pair: SocketPair, started: Instant, now: Instant) -> Option<(u64, bool)> {
        self.entries
            .retain(|entry| now.saturating_duration_since(entry.3) < CONNECTION_HISTORY_TTL);
        if let Some(index) = self.entries.iter().position(|entry| entry.0 == pair) {
            let mut entry = self.entries.remove(index)?;
            let result = (entry.1, entry.2 < started);
            entry.3 = now;
            self.entries.push_back(entry);
            return Some(result);
        }
        self.next_id = self.next_id.checked_add(1)?;
        if self.entries.len() == MAX_CONNECTION_HISTORY {
            self.entries.pop_front();
        }
        self.entries.push_back((pair, self.next_id, now, now));
        Some((self.next_id, false))
    }
}
#[derive(Clone, Default)]
pub struct ConnectionTracker(Arc<Mutex<History>>);
pub fn observable_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
#[derive(Clone)]
pub struct ObserveContext {
    id: String,
    sink: Arc<dyn TransportObserver>,
    start: Instant,
    tracker: ConnectionTracker,
}
impl ObserveContext {
    pub fn new(id: String, sink: Arc<dyn TransportObserver>) -> Self {
        Self { id, sink, start: Instant::now(), tracker: ConnectionTracker::default() }
    }
    pub fn with_tracker(mut self, tracker: ConnectionTracker) -> Self {
        self.tracker = tracker;
        self
    }
    pub fn connection(&self, response: &reqwest::Response) {
        let Some(info) =
            response.extensions().get::<hyper_util::client::legacy::connect::HttpInfo>()
        else {
            self.event("south_connection_unknown");
            return;
        };
        let Ok(mut seen) = self.tracker.0.lock() else {
            self.event("south_connection_unknown");
            return;
        };
        let result =
            seen.record((info.local_addr(), info.remote_addr()), self.start, Instant::now());
        drop(seen);
        let Some((fingerprint, reused)) = result else {
            self.event("south_connection_unknown");
            return;
        };
        self.sink.connection(
            &self.id,
            self.start.elapsed(),
            fingerprint,
            reused,
            &format!("{:?}", response.version()),
        );
    }
    pub fn event(&self, stage: &'static str) {
        self.sink.event(&self.id, stage, self.start.elapsed());
    }
}
pub struct ObservedBody {
    bytes: Option<Bytes>,
    context: ObserveContext,
}
impl ObservedBody {
    pub const fn new(bytes: Bytes, context: ObserveContext) -> Self {
        Self { bytes: Some(bytes), context }
    }
}
impl http_body::Body for ObservedBody {
    type Data = Bytes;
    type Error = Infallible;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Bytes>, Infallible>>> {
        if let Some(bytes) = self.bytes.take() {
            self.context.event("south_body_first_poll");
            self.context.event("south_body_handed_off");
            Poll::Ready(Some(Ok(http_body::Frame::data(bytes))))
        } else {
            Poll::Ready(None)
        }
    }
    fn is_end_stream(&self) -> bool {
        self.bytes.is_none()
    }
    fn size_hint(&self) -> http_body::SizeHint {
        http_body::SizeHint::with_exact(
            u64::try_from(self.bytes.as_ref().map_or(0, Bytes::len)).unwrap_or(u64::MAX),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_is_bounded_expires_and_never_reuses_evicted_ids() {
        let now = Instant::now();
        let mut history = History::default();
        let pair = |port| {
            (SocketAddr::from(([127, 0, 0, 1], port)), SocketAddr::from(([127, 0, 0, 1], 80)))
        };
        for index in 0..300_u16 {
            assert_eq!(history.record(pair(index), now, now), Some((u64::from(index) + 1, false)));
        }
        assert_eq!(history.entries.len(), MAX_CONNECTION_HISTORY);
        assert_eq!(history.record(pair(0), now, now), Some((301, false)));
        let later = now + Duration::from_secs(61);
        assert_eq!(history.record(pair(0), later, later), Some((302, false)));
        assert_eq!(history.entries.len(), 1);
        assert_eq!(
            history.record(
                pair(0),
                later + Duration::from_millis(1),
                later + Duration::from_millis(2)
            ),
            Some((302, true))
        );
    }
    #[test]
    fn exhausted_identifiers_stay_unknown() {
        let now = Instant::now();
        let mut history = History { next_id: u64::MAX, ..History::default() };
        let pair = (SocketAddr::from(([127, 0, 0, 1], 1)), SocketAddr::from(([127, 0, 0, 1], 80)));
        assert_eq!(history.record(pair, now, now), None);
        assert!(history.entries.is_empty());
    }
    #[test]
    fn opaque_ids_reject_unbounded_and_sensitive_shaped_values() {
        assert!(observable_id("req_0123-abc"));
        assert!(observable_id(&"x".repeat(128)));
        for id in ["", "secret@example.invalid", "contains space", &"x".repeat(129)] {
            assert!(!observable_id(id));
        }
    }
}
