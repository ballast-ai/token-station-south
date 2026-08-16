use std::sync::Arc;

use south_contracts::{BufferedHttpResponseV1, JsonBodyV1, JsonPostRequestV1};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(JsonBodyV1: Clone);
assert_not_impl_any!(JsonPostRequestV1: Clone);
assert_not_impl_any!(BufferedHttpResponseV1: Clone);

#[test]
fn body_bearing_contracts_have_single_owner_semantics() {}

#[test]
fn json_body_exposes_only_a_shared_owner_without_copying_text() {
    let body = JsonBodyV1::parse(r#"{"input":"shared-owner-sentinel"}"#)
        .expect("fixture body should be valid");

    let first = body.shared_owner();
    let second = body.shared_owner();

    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(first.as_ptr(), body.as_str().as_ptr());
    assert_eq!(first.as_ref(), body.as_str());
}
