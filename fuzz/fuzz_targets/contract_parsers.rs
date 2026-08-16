#![no_main]

use libfuzzer_sys::fuzz_target;
use south_contracts::{
    CredentialSlotV1, JsonBodyV1, MAX_CREDENTIAL_SLOT_BYTES, MAX_ENDPOINT_BYTES,
    MAX_JSON_REQUEST_BODY_BYTES, MAX_RELATIVE_PATH_BYTES, ProviderEndpointV1, RelativePathV1,
};

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
});
