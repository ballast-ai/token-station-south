//! Compile the explicit local-write constructor from an independent final Cargo root.
use south_transport_reqwest::{ReqwestTransportConfigV1, TransportObserver, TransportPairV1};
use std::{sync::Arc, time::Duration};
struct Sink;
impl TransportObserver for Sink {
    fn event(&self, _: &str, _: &'static str, _: Duration) {}
    fn local_write(&self, _: &str, _: &'static str, _: u64) {}
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ReqwestTransportConfigV1::try_new(
        Duration::from_secs(10),
        Duration::from_secs(2),
        Duration::from_secs(5),
    )?;
    let pair = TransportPairV1::try_new_local_write_observed(config, Arc::new(Sink))?;
    fn accepts_transport<T: south_core::AsyncHttpTransport>(_: &T) {}
    accepts_transport(&pair.buffered);
    let _: Result<ReqwestTransportConfigV1, south_contracts::TransportErrorV1> = Ok(config);
    drop(pair);
    Ok(())
}
