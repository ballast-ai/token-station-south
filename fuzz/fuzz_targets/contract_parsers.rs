#![no_main]

use libfuzzer_sys::fuzz_target;
use south_contracts::{
    CredentialSlotV1, JsonBodyV1, MAX_CREDENTIAL_SLOT_BYTES, MAX_ENDPOINT_BYTES,
    MAX_JSON_REQUEST_BODY_BYTES, MAX_PROVIDER_QUOTA_METADATA_TOTAL_BYTES,
    MAX_PROVIDER_QUOTA_METADATA_VALUE_BYTES, MAX_QUERY_TOTAL_BYTES, MAX_RELATIVE_PATH_BYTES,
    ProviderEndpointV1, ProviderQuotaMetadataFieldV1, ProviderQuotaMetadataV1, QueryParameterV1,
    QueryStringV1, RelativePathV1,
};

const QUOTA_FIELDS: [ProviderQuotaMetadataFieldV1; 9] = [
    ProviderQuotaMetadataFieldV1::XRateLimitLimitTokens,
    ProviderQuotaMetadataFieldV1::XRateLimitRemainingTokens,
    ProviderQuotaMetadataFieldV1::XRateLimitResetTokens,
    ProviderQuotaMetadataFieldV1::AnthropicRateLimitTokensLimit,
    ProviderQuotaMetadataFieldV1::AnthropicRateLimitTokensRemaining,
    ProviderQuotaMetadataFieldV1::AnthropicRateLimitTokensReset,
    ProviderQuotaMetadataFieldV1::AnthropicRateLimitUnifiedLimit,
    ProviderQuotaMetadataFieldV1::AnthropicRateLimitUnifiedRemaining,
    ProviderQuotaMetadataFieldV1::AnthropicRateLimitUnifiedReset,
];

fuzz_target!(|data: &[u8]| {
    let Ok(input) = std::str::from_utf8(data) else {
        return;
    };

    if let Ok(endpoint) = ProviderEndpointV1::parse(input) {
        assert!(endpoint.as_str().len() <= MAX_ENDPOINT_BYTES);
        assert!(endpoint.as_str().ends_with('/'));
        assert_eq!(ProviderEndpointV1::parse(endpoint.as_str()), Ok(endpoint));
    }

    if let Ok(path) = RelativePathV1::parse(input) {
        assert!(!path.as_str().is_empty());
        assert!(path.as_str().is_ascii());
        assert!(path.as_str().len() <= MAX_RELATIVE_PATH_BYTES);
        for (binding, scheme, effective_port, base_path) in [
            ("https://example.com/", "https", 443, "/"),
            ("https://example.com/base/", "https", 443, "/base/"),
            ("https://example.com/base%3Av1/", "https", 443, "/base%3Av1/"),
            ("https://example.com:8443/base/", "https", 8443, "/base/"),
        ] {
            let Ok(endpoint) = ProviderEndpointV1::parse(binding) else {
                panic!("static fuzz binding must be valid");
            };
            let Ok(resolved) = path.resolve_against(&endpoint) else {
                panic!("accepted relative path must remain inside a valid binding");
            };
            assert_eq!(resolved.scheme(), scheme);
            assert_eq!(resolved.host_str(), Some("example.com"));
            assert_eq!(resolved.port_or_known_default(), Some(effective_port));
            assert!(resolved.path().starts_with(base_path));
        }
        assert_eq!(RelativePathV1::parse(path.as_str()), Ok(path));
    }

    // Controlled query: a successfully constructed query must survive the join byte for byte,
    // against every valid binding, exactly like an accepted path must resolve.
    for parameter in QueryParameterV1::ALL {
        let Ok(query) = QueryStringV1::try_from_iter([(parameter, input)]) else {
            continue;
        };
        assert!(!query.as_str().is_empty());
        assert!(query.as_str().len() <= MAX_QUERY_TOTAL_BYTES);
        assert!(query.as_str().is_ascii());
        // A sanctioned name is always present, and no value may smuggle a separator that would
        // let one declaration masquerade as two.
        assert!(query.as_str().starts_with(parameter.wire_name()));
        assert_eq!(query.as_str().matches('&').count(), 0);
        assert_eq!(query.as_str().matches('=').count(), 1);
        assert!(!query.as_str().contains('#'));

        let Ok(path) = RelativePathV1::parse("v1/resource") else {
            panic!("static fuzz path must be valid");
        };
        for binding in ["https://example.com/", "https://example.com/base/"] {
            let Ok(endpoint) = ProviderEndpointV1::parse(binding) else {
                panic!("static fuzz binding must be valid");
            };
            let Ok(resolved) = path.resolve_against_with_query(&endpoint, Some(&query)) else {
                panic!("accepted query must remain inside a valid binding");
            };
            assert_eq!(resolved.query(), Some(query.as_str()));
            assert_eq!(resolved.host_str(), Some("example.com"));
            assert!(resolved.fragment().is_none());
        }
    }

    if let Ok(slot) = CredentialSlotV1::parse(input) {
        assert!(!slot.as_str().is_empty());
        assert!(slot.as_str().is_ascii());
        assert!(slot.as_str().len() <= MAX_CREDENTIAL_SLOT_BYTES);
        assert_eq!(CredentialSlotV1::parse(slot.as_str()), Ok(slot));
    }

    if let Ok(body) = JsonBodyV1::parse(input) {
        assert_eq!(body.as_str(), input);
        assert!(body.len() <= MAX_JSON_REQUEST_BODY_BYTES);
        assert_eq!(JsonBodyV1::parse(body.as_str()), Ok(body));
    }

    let fields = input
        .split('\0')
        .enumerate()
        .map(|(index, value)| (QUOTA_FIELDS[index % QUOTA_FIELDS.len()], value.to_owned()))
        .collect::<Vec<_>>();
    if let Ok(metadata) = ProviderQuotaMetadataV1::try_from_iter(fields) {
        let present = QUOTA_FIELDS
            .into_iter()
            .filter_map(|field| metadata.value(field).map(|value| (field, value)))
            .collect::<Vec<_>>();
        assert_eq!(present.len(), metadata.present_field_count());
        assert!(present.len() <= QUOTA_FIELDS.len());
        assert!(
            present
                .iter()
                .all(|(_, value)| value.len() <= MAX_PROVIDER_QUOTA_METADATA_VALUE_BYTES)
        );
        assert!(
            present.iter().map(|(_, value)| value.len()).sum::<usize>()
                <= MAX_PROVIDER_QUOTA_METADATA_TOTAL_BYTES
        );
        let rebuilt = ProviderQuotaMetadataV1::try_from_iter(
            present.into_iter().map(|(field, value)| (field, value.to_owned())),
        );
        assert_eq!(rebuilt, Ok(metadata));
    }
});
