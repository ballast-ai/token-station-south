//! `MiniMax`'s second artifact GET needs one bounded, non-secret query field.
use south_contracts::{ContractErrorV1, MAX_QUERY_VALUE_BYTES, QueryParameterV1, QueryStringV1};
#[test]
fn file_id_query_is_bounded_decimal_and_preserves_original_digits() {
    let parameter = QueryParameterV1::ALL
        .into_iter()
        .find(|p| p.wire_name() == "file_id")
        .expect("file retrieval query must be sanctioned");
    let q = QueryStringV1::try_from_iter([
        (parameter, "00176844028768320"),
        (QueryParameterV1::GroupId, "19000"),
    ])
    .unwrap();
    assert_eq!(q.as_str(), "GroupId=19000&file_id=00176844028768320");
    for id in ["", "-1", "1.0", "a", "1&key=secret", "1%26x", "１２"] {
        assert_eq!(
            QueryStringV1::try_from_iter([(parameter, id)]),
            Err(ContractErrorV1::InvalidQueryValue)
        );
    }
    assert!(
        QueryStringV1::try_from_iter([(parameter, "1".repeat(MAX_QUERY_VALUE_BYTES).as_str())])
            .is_ok()
    );
    assert!(
        QueryStringV1::try_from_iter([(parameter, "1".repeat(MAX_QUERY_VALUE_BYTES + 1).as_str())])
            .is_err()
    );
    assert_eq!(
        QueryStringV1::try_from_iter([(parameter, "1"), (parameter, "2")]),
        Err(ContractErrorV1::DuplicateQueryParameter)
    );
}
