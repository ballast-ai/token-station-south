use proptest::{
    prelude::*,
    test_runner::{Config, RngSeed},
};
use south_contracts::{
    CredentialSlotV1, JsonBodyV1, MAX_CREDENTIAL_SLOT_BYTES, MAX_ENDPOINT_BYTES,
    MAX_JSON_REQUEST_BODY_BYTES, MAX_RELATIVE_PATH_BYTES, ProviderEndpointV1, RelativePathV1,
};

fn reproducible_config(seed: u64) -> Config {
    Config {
        cases: 256,
        failure_persistence: None,
        rng_seed: RngSeed::Fixed(seed),
        ..Config::default()
    }
}

fn ascii_string(
    alphabet: &'static [u8],
    lengths: std::ops::Range<usize>,
) -> impl Strategy<Value = String> {
    proptest::collection::vec(proptest::sample::select(alphabet), lengths)
        .prop_map(|bytes| bytes.into_iter().map(char::from).collect())
}

fn valid_endpoint_inputs() -> impl Strategy<Value = String> {
    (
        prop_oneof![Just("http"), Just("https")],
        ascii_string(b"abcdefghijklmnopqrstuvwxyz", 1..17),
        proptest::collection::vec(
            ascii_string(b"abcdefghijklmnopqrstuvwxyz0123456789_-", 1..13),
            0..5,
        ),
        any::<bool>(),
    )
        .prop_map(|(scheme, host, segments, trailing_slash)| {
            let mut endpoint = format!("{scheme}://{host}.example");
            if !segments.is_empty() {
                endpoint.push('/');
                endpoint.push_str(&segments.join("/"));
                if trailing_slash {
                    endpoint.push('/');
                }
            }
            endpoint
        })
}

fn valid_relative_path_inputs() -> impl Strategy<Value = String> {
    proptest::collection::vec(ascii_string(b"abcdefghijklmnopqrstuvwxyz0123456789_-", 1..17), 1..6)
        .prop_map(|segments| segments.join("/"))
}

fn valid_credential_slot_inputs() -> impl Strategy<Value = String> {
    (
        proptest::sample::select(b"abcdefghijklmnopqrstuvwxyz"),
        ascii_string(b"abcdefghijklmnopqrstuvwxyz0123456789._-", 0..MAX_CREDENTIAL_SLOT_BYTES),
    )
        .prop_map(|(first, rest)| format!("{}{rest}", char::from(first)))
}

fn valid_json_inputs() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("null".to_owned()),
        any::<bool>().prop_map(|value| value.to_string()),
        any::<i64>().prop_map(|value| value.to_string()),
        any::<String>().prop_map(|value| {
            serde_json::to_string(&value).unwrap_or_else(|_| "null".to_owned())
        }),
    ]
}

proptest! {
    #![proptest_config(reproducible_config(0x45_4e_44_50_4f_49_4e_54))]

    #[test]
    fn accepted_endpoints_are_bounded_canonical_and_idempotent(
        input in prop_oneof![any::<String>(), valid_endpoint_inputs()],
    ) {
        if let Ok(endpoint) = ProviderEndpointV1::parse(&input) {
            let canonical = endpoint.as_str();
            prop_assert!(canonical.len() <= MAX_ENDPOINT_BYTES);
            prop_assert!(canonical.starts_with("http://") || canonical.starts_with("https://"));
            prop_assert!(canonical.ends_with('/'));
            prop_assert_eq!(ProviderEndpointV1::parse(canonical), Ok(endpoint));
        }
    }
}

proptest! {
    #![proptest_config(reproducible_config(0x52_45_4c_41_54_49_56_45))]

    #[test]
    fn accepted_relative_paths_are_bounded_and_resolve_inside_the_binding(
        input in prop_oneof![any::<String>(), valid_relative_path_inputs()],
    ) {
        if let Ok(path) = RelativePathV1::parse(&input) {
            prop_assert!(!path.as_str().is_empty());
            prop_assert!(path.as_str().is_ascii());
            prop_assert!(path.as_str().len() <= MAX_RELATIVE_PATH_BYTES);
            prop_assert_eq!(RelativePathV1::parse(path.as_str()), Ok(path.clone()));

            let endpoint = ProviderEndpointV1::parse("https://example.com/base/")?;
            let resolved = path.resolve_against(&endpoint)?;
            prop_assert_eq!(resolved.scheme(), "https");
            prop_assert_eq!(resolved.host_str(), Some("example.com"));
            prop_assert_eq!(resolved.port_or_known_default(), Some(443));
            prop_assert!(resolved.path().starts_with("/base/"));
        }
    }
}

proptest! {
    #![proptest_config(reproducible_config(0x43_52_45_44_45_4e_54_49))]

    #[test]
    fn accepted_credential_slots_match_the_complete_ascii_grammar(
        input in prop_oneof![any::<String>(), valid_credential_slot_inputs()],
    ) {
        if let Ok(slot) = CredentialSlotV1::parse(&input) {
            let mut bytes = slot.as_str().bytes();
            prop_assert!(slot.as_str().len() <= MAX_CREDENTIAL_SLOT_BYTES);
            prop_assert!(bytes.next().is_some_and(|byte| byte.is_ascii_lowercase()));
            let remaining_bytes_are_valid = bytes.all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'_' | b'-')
            });
            prop_assert!(remaining_bytes_are_valid);
            prop_assert_eq!(CredentialSlotV1::parse(slot.as_str()), Ok(slot));
        }
    }
}

proptest! {
    #![proptest_config(reproducible_config(0x4a_53_4f_4e_42_4f_44_59))]

    #[test]
    fn accepted_json_bodies_retain_exactly_one_bounded_value(
        input in prop_oneof![any::<String>(), valid_json_inputs()],
    ) {
        if let Ok(body) = JsonBodyV1::parse(&input) {
            prop_assert_eq!(body.as_str(), input.as_str());
            prop_assert!(body.len() <= MAX_JSON_REQUEST_BODY_BYTES);
            prop_assert!(serde_json::from_str::<serde_json::Value>(body.as_str()).is_ok());
            prop_assert_eq!(JsonBodyV1::parse(body.as_str()), Ok(body));
        }
    }
}
