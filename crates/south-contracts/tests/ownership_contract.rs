use south_contracts::{BufferedHttpResponseV1, JsonBodyV1, JsonPostRequestV1};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(JsonBodyV1: Clone);
assert_not_impl_any!(JsonPostRequestV1: Clone);
assert_not_impl_any!(BufferedHttpResponseV1: Clone);

#[test]
fn body_bearing_contracts_have_single_owner_semantics() {}
