//! A bounded, connection-local HTTP/1.1 write tracker.
//!
//! This module has no transport, clock, logging, or external dependencies. It consumes only
//! bytes that the connection reports as accepted. Completion remains local to the client.

const MAX_HEADER_BYTES: usize = 65_536;
const MAX_REQUEST_ID_BYTES: usize = 128;

#[derive(Default)]
pub struct WriteTracker {
    header: Vec<u8>,
    body: Option<(String, u64, u64)>,
    complete: Option<(String, u64)>,
    disabled: bool,
}

impl WriteTracker {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn abort(&mut self) {
        self.clear_header();
        self.body = None;
        self.complete = None;
        self.disabled = true;
    }

    fn clear_header(&mut self) {
        // Explicit best-effort clearing; this is not a strong zeroization guarantee.
        self.header.fill(0);
        self.header.clear();
    }

    pub(crate) fn accepted(&mut self, mut data: &[u8]) -> Option<(String, u64)> {
        if self.disabled || data.is_empty() {
            return None;
        }
        if self.complete.is_some() {
            self.abort();
            return None;
        }
        while self.body.is_none() && !data.is_empty() {
            if self.header.len() == MAX_HEADER_BYTES {
                self.abort();
                return None;
            }
            self.header.push(data[0]);
            data = &data[1..];
            if self.header.ends_with(b"\r\n\r\n") {
                let parsed = parse_header(&self.header);
                self.clear_header();
                let Some((id, length)) = parsed else {
                    self.abort();
                    return None;
                };
                self.body = Some((id, length, 0));
            }
        }
        if let Some((id, length, written)) = &mut self.body {
            let Ok(accepted) = u64::try_from(data.len()) else {
                self.abort();
                return None;
            };
            if accepted > *length - *written {
                self.abort();
                return None;
            }
            *written += accepted;
            if written == length {
                let event = (id.clone(), *length);
                self.complete = Some(event.clone());
                self.body = None;
                return Some(event);
            }
        }
        None
    }

    pub(crate) const fn flushed(&mut self) -> Option<(String, u64)> {
        self.complete.take()
    }
}

impl Drop for WriteTracker {
    fn drop(&mut self) {
        self.abort();
    }
}

fn parse_header(bytes: &[u8]) -> Option<(String, u64)> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.split("\r\n");
    let first = lines.next()?;
    if !first.starts_with("POST ") || !first.ends_with(" HTTP/1.1") {
        return None;
    }
    let mut id = None;
    let mut length = None;
    for line in lines.filter(|s| !s.is_empty()) {
        let (name, value) = line.split_once(':')?;
        // This hook targets the pinned writer's single encoding SP, not a general HTTP reader.
        // Preserve any additional caller whitespace so it cannot create a new correlation ID.
        let value = if name.eq_ignore_ascii_case("x-request-id") {
            value.strip_prefix(' ')?
        } else {
            value.trim()
        };
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return None;
        }
        if name.eq_ignore_ascii_case("x-request-id") {
            if id.is_some()
                || value.is_empty()
                || value.len() > MAX_REQUEST_ID_BYTES
                || !value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                return None;
            }
            id = Some(value.to_owned());
        }
        if name.eq_ignore_ascii_case("content-length") {
            if length.is_some() || value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            length = Some(value.parse::<u64>().ok()?);
        }
    }
    Some((id?, length?))
}

#[cfg(test)]
mod tests {
    use super::WriteTracker;
    fn header(extra: &str) -> String {
        format!("POST / HTTP/1.1\r\nx-request-id: req_mock\r\nContent-Length: 3\r\n{extra}\r\n")
    }
    #[test]
    fn partial_acceptance_and_premature_flush_do_not_complete() {
        let mut tracker = WriteTracker::new();
        for b in header("").bytes() {
            assert!(tracker.accepted(&[b]).is_none());
        }
        assert!(tracker.accepted(b"ab").is_none());
        assert!(tracker.flushed().is_none());
        assert_eq!(tracker.accepted(b"c"), Some(("req_mock".into(), 3)));
        assert_eq!(tracker.flushed(), Some(("req_mock".into(), 3)));
        assert!(tracker.flushed().is_none());
    }
    #[test]
    fn invalid_framing_and_abort_do_not_report_flush() {
        for raw in [
            header("Transfer-Encoding: chunked\r\n"),
            header("Content-Length: 3\r\n"),
            "POST / HTTP/2\r\nx-request-id: req_mock\r\nContent-Length: 3\r\n\r\n".into(),
        ] {
            let mut tracker = WriteTracker::new();
            assert!(tracker.accepted(raw.as_bytes()).is_none());
            assert!(tracker.accepted(b"abc").is_none());
            assert!(tracker.flushed().is_none());
        }
        let mut tracker = WriteTracker::new();
        tracker.accepted(header("").as_bytes());
        assert!(tracker.accepted(b"abc").is_some());
        tracker.abort();
        assert!(tracker.flushed().is_none());
    }
    #[test]
    fn keepalive_resets_without_duplicate_events() {
        let mut tracker = WriteTracker::new();
        for _ in 0..2 {
            tracker.accepted(header("Authorization: Bearer fixture\r\n").as_bytes());
            assert!(tracker.accepted(b"abc").is_some());
            assert!(tracker.flushed().is_some());
            assert!(tracker.flushed().is_none());
        }
        assert!(tracker.header.is_empty());
    }
    #[test]
    fn oversized_header_and_interrupted_header_are_cleared() {
        let mut tracker = WriteTracker::new();
        assert!(tracker.accepted(&vec![b'a'; 65537]).is_none());
        assert!(tracker.header.is_empty());
        assert!(tracker.accepted(header("").as_bytes()).is_none());
        let mut tracker = WriteTracker::new();
        tracker.accepted(b"POST / HTTP/1.1\r\nAuthorization: Bearer fixture");
        tracker.abort();
        assert!(tracker.header.is_empty());
        assert!(tracker.flushed().is_none());
    }
    #[test]
    fn overrun_pipelining_and_empty_ids_are_unobservable() {
        let mut tracker = WriteTracker::new();
        tracker.accepted(header("").as_bytes());
        assert!(tracker.accepted(b"abcd").is_none());
        assert!(tracker.flushed().is_none());
        let mut tracker = WriteTracker::new();
        tracker.accepted(header("").as_bytes());
        tracker.accepted(b"abc");
        tracker.accepted(header("").as_bytes());
        assert!(tracker.flushed().is_none());
        let mut tracker = WriteTracker::new();
        assert!(tracker.accepted(header("").replace("req_mock", "").as_bytes()).is_none());
        assert!(tracker.accepted(b"abc").is_none());
    }
    #[test]
    fn request_id_whitespace_is_not_normalized_into_a_new_correlation() {
        for id in [" req_mock", "req_mock ", "\treq_mock", "req_mock\t"] {
            let mut tracker = WriteTracker::new();
            let wire =
                format!("POST / HTTP/1.1\r\nx-request-id: {id}\r\nContent-Length: 0\r\n\r\n");
            assert!(tracker.accepted(wire.as_bytes()).is_none());
            assert!(tracker.flushed().is_none());
        }
    }
}
