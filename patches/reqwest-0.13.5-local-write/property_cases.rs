// Included by South's verification harness with WriteTracker in scope and proptest as a dev-dependency.
use proptest::prelude::*;

proptest! {
    #[test]
    fn complete_request_is_independent_of_accepted_chunk_sizes(
        body in prop::collection::vec(any::<u8>(),0..4096),
        chunk_sizes in prop::collection::vec(1usize..512,1..32),
    ) {
        let header=format!("POST / HTTP/1.1\r\nx-request-id: req_property\r\nContent-Length: {}\r\n\r\n",body.len());
        let mut wire=header.into_bytes();wire.extend_from_slice(&body);
        let mut tracker=WriteTracker::new();let mut position=0;let mut event=None;let mut i=0;
        while position<wire.len() {
            let end=(position+chunk_sizes[i%chunk_sizes.len()]).min(wire.len());
            let next=tracker.accepted(&wire[position..end]);
            if next.is_some() {prop_assert!(event.is_none());event=next;}
            position=end;i+=1;
            if position<wire.len() {prop_assert!(tracker.flushed().is_none());}
        }
        let expected=Some(("req_property".to_owned(),u64::try_from(body.len()).unwrap()));
        prop_assert_eq!(&event,&expected);let flushed=tracker.flushed();prop_assert_eq!(&flushed,&expected);prop_assert!(tracker.flushed().is_none());
    }

    #[test]
    fn arbitrary_interrupted_prefix_never_completes_flush(
        bytes in prop::collection::vec(any::<u8>(),0..8192),
        chunks in 1usize..128,
    ) {
        let mut tracker=WriteTracker::new();
        for part in bytes.chunks(chunks) {let _=tracker.accepted(part);}
        tracker.abort();prop_assert!(tracker.flushed().is_none());
        prop_assert!(tracker.accepted(b"POST / HTTP/1.1\r\nx-request-id: req_x\r\nContent-Length: 0\r\n\r\n").is_none());
    }
}
