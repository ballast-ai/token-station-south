#![no_main]

use libfuzzer_sys::fuzz_target;
use south_contracts::{
    DeclaredSecretHeaderV1, DeclaredSecretHeadersV1, MAX_SECRET_HEADER_NAME_BYTES,
    ResponseTranscriptV1, SafeHeaders,
};

fuzz_target!(|data: &[u8]| {
    let split = data.len() / 2;
    if let (Ok(name), Ok(value)) = (
        std::str::from_utf8(&data[..split]),
        std::str::from_utf8(&data[split..]),
    ) {
        let _result = SafeHeaders::try_from_iter([(name, value)]);

        // A declared secret header name (auth contract version five): an accepted name is a
        // bounded lowercase token that round-trips, and is then reserved on the ordinary channel
        // and dropped from the transcript of the declaring package.
        if let Ok(declared) = DeclaredSecretHeaderV1::parse(name) {
            assert_eq!(declared.as_str(), name);
            assert!(!name.is_empty() && name.len() <= MAX_SECRET_HEADER_NAME_BYTES);
            assert!(!name.bytes().any(|byte| byte.is_ascii_uppercase()));
            let package = DeclaredSecretHeadersV1::try_new([declared])
                .expect("one valid name is a valid declaration");
            assert!(
                SafeHeaders::try_from_iter_with_secret_headers([(name, value)], &package).is_err()
            );
            let transcript =
                ResponseTranscriptV1::capture_redacting([(name, Some(value))], &package);
            assert!(transcript.iter().all(|(captured, _)| captured != name));
        }

        // Every value-side name list parses or refuses without panicking.
        let names: Vec<&str> = value.split(',').collect();
        if let Ok(package) = DeclaredSecretHeadersV1::try_from_names(names.iter().copied()) {
            let _result = SafeHeaders::try_from_iter_with_secret_headers([(name, value)], &package);
        }
    }
});
