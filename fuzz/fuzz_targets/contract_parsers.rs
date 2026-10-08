#![no_main]

use libfuzzer_sys::fuzz_target;
use south_component_conformance::task_v2_json::{
    locator_json, observation_json, parse_locator_json, parse_observation_json,
    parse_prepared_task_json, parse_render_context_json, parse_submit_outcome_json,
    prepared_task_json, render_context_json, submit_outcome_json,
};
use south_contracts::{
    AwsEventStreamDeframerV1, ControlledUserAgentV1, CredentialSlotV1, DeclaredQueryParameterV1,
    DeclaredUserAgentV1, JsonBodyV1, MAX_CREDENTIAL_SLOT_BYTES, MAX_DECLARED_QUERY_NAME_BYTES,
    MAX_ENDPOINT_BYTES, MAX_JSON_REQUEST_BODY_BYTES, MAX_PROVIDER_QUOTA_METADATA_TOTAL_BYTES,
    MAX_PROVIDER_QUOTA_METADATA_VALUE_BYTES, MAX_QUERY_TOTAL_BYTES, MAX_QUOTA_HEADER_NAME_BYTES,
    MAX_RELATIVE_PATH_BYTES, MAX_RESPONSE_DIAGNOSTIC_TOTAL_BYTES,
    MAX_RESPONSE_DIAGNOSTIC_VALUE_BYTES, MAX_RESPONSE_TRANSCRIPT_COUNT,
    MAX_RESPONSE_TRANSCRIPT_NAME_BYTES, MAX_RESPONSE_TRANSCRIPT_TOTAL_BYTES,
    MAX_RESPONSE_TRANSCRIPT_VALUE_BYTES, MAX_USER_AGENT_BYTES, PROVIDER_QUOTA_HEADER_DENIED_NAMES,
    ProviderEndpointV1, ProviderQuotaHeaderMapV1, ProviderQuotaMetadataFieldV1,
    ProviderQuotaMetadataV1, QueryParameterV1, QueryStringV1, QueryValueSyntaxV1,
    RESPONSE_DIAGNOSTIC_FIELD_COUNT, RelativePathV1, ResponseDiagnosticFieldV1,
    ResponseDiagnosticsV1, ResponseTranscriptV1, deframe_aws_eventstream_v1,
    reencode_eventstream_v1,
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

/// Bitwise CRC32 (IEEE), so constructed frames reach the header and payload grammar instead of
/// stopping at the checksums, which random bytes almost never satisfy.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn eventstream_frame(header_block: &[u8], payload: &[u8]) -> Vec<u8> {
    let length = |len: usize| u32::try_from(len).expect("fuzz inputs are small").to_be_bytes();
    let mut frame = Vec::new();
    frame.extend_from_slice(&length(16 + header_block.len() + payload.len()));
    frame.extend_from_slice(&length(header_block.len()));
    frame.extend_from_slice(&crc32(&frame).to_be_bytes());
    frame.extend_from_slice(header_block);
    frame.extend_from_slice(payload);
    frame.extend_from_slice(&crc32(&frame).to_be_bytes());
    frame
}

/// The AWS eventstream deframer and canonical re-encoding (host-zero-vendor-boundary §5.2).
fn fuzz_eventstream(data: &[u8]) {
    // Raw upstream bytes: chunking never changes the messages or the first error.
    let whole = deframe_aws_eventstream_v1(data);
    let stride = usize::from(data.first().copied().unwrap_or(0) % 17) + 1;
    let mut deframer = AwsEventStreamDeframerV1::new();
    let mut messages = Vec::new();
    let mut outcome = Ok(());
    'chunks: for chunk in data.chunks(stride) {
        deframer.push(chunk);
        loop {
            match deframer.next_message() {
                Ok(Some(message)) => messages.push(message),
                Ok(None) => break,
                Err(error) => {
                    outcome = Err(error);
                    break 'chunks;
                }
            }
        }
    }
    if outcome.is_ok() {
        outcome = deframer.finish();
    }
    match whole {
        Ok(expected) => {
            assert_eq!(outcome, Ok(()));
            assert_eq!(messages, expected);
        }
        Err(error) => assert_eq!(outcome, Err(error)),
    }

    // A checksum-valid frame around fuzzed headers and payload, behind a fuzz-selected message
    // type, exercises the header grammar and the re-encoding.
    let Some((&selector, rest)) = data.split_first() else {
        return;
    };
    let split = usize::from(selector) % (rest.len() + 1);
    let (fuzzed_headers, payload) = rest.split_at(split);
    let message_type: &[u8] = match selector % 3 {
        0 => b"event",
        1 => b"exception",
        _ => b"error",
    };
    let mut header_block = vec![13];
    header_block.extend_from_slice(b":message-type");
    header_block.push(7);
    header_block
        .extend_from_slice(&u16::try_from(message_type.len()).expect("short").to_be_bytes());
    header_block.extend_from_slice(message_type);
    header_block.extend_from_slice(fuzzed_headers);
    let Ok(decoded) = deframe_aws_eventstream_v1(&eventstream_frame(&header_block, payload)) else {
        return;
    };
    assert_eq!(decoded.len(), 1);
    if let Ok(frame) = reencode_eventstream_v1(&decoded[0]) {
        // Exactly one SSE frame: the `event:` line, the `data:` line, the blank line.
        assert!(frame.starts_with("event: "));
        assert!(frame.ends_with("\n\n"));
        assert_eq!(frame.matches('\n').count(), 3);
        assert!(!frame.contains('\r'));
        assert_eq!(frame.lines().nth(1).map(|line| line.starts_with("data: ")), Some(true));
    }
}

/// B7a: the declared-instance parsers consume untrusted manifest text (§10, §16 Q15).
fn fuzz_declared_instances(input: &str) {
    // A declared user-agent: an accepted value has the controlled grammar and round-trips.
    if let Ok(agent) = DeclaredUserAgentV1::from_manifest_value(input) {
        assert_eq!(agent.as_str(), input);
        assert!(!input.is_empty() && input.len() <= MAX_USER_AGENT_BYTES);
        assert!(input.bytes().all(|byte| (0x20..=0x7e).contains(&byte)));
        assert!(!input.starts_with(' ') && !input.ends_with(' '));
        let literal: &'static str = Box::leak(input.to_owned().into_boxed_str());
        assert!(ControlledUserAgentV1::try_from_static(literal).is_ok());
    }

    // A declared query parameter: `name\0value\0enum values...`. An accepted name with an accepted
    // value yields exactly one `name=value` pair that survives the join byte for byte.
    let mut parts = input.split('\0');
    let name = parts.next().unwrap_or_default();
    let value = parts.next().unwrap_or_default();
    let listed: Vec<String> = parts.map(str::to_owned).collect();
    for syntax in [
        QueryValueSyntaxV1::Digits,
        QueryValueSyntaxV1::Token,
        QueryValueSyntaxV1::Date,
        QueryValueSyntaxV1::Enum(listed),
    ] {
        let Ok(parameter) = DeclaredQueryParameterV1::try_new(name, syntax) else {
            continue;
        };
        assert_eq!(parameter.name(), name);
        assert!(!name.is_empty() && name.len() <= MAX_DECLARED_QUERY_NAME_BYTES);
        assert!(name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"-._~".contains(&byte)));
        assert!(!QueryParameterV1::ALL.iter().any(|fixed| fixed.wire_name() == name));
        let Ok(query) =
            QueryStringV1::try_from_iter([(QueryParameterV1::Declared(parameter), value)])
        else {
            continue;
        };
        assert_eq!(query.as_str(), format!("{name}={value}"));
        assert_eq!(query.as_str().matches('=').count(), 1);
        assert!(!query.as_str().contains(['&', '#', '%', '+', ' ']));
        let (Ok(path), Ok(endpoint)) = (
            RelativePathV1::parse("v1/resource"),
            ProviderEndpointV1::parse("https://example.com/base/"),
        ) else {
            panic!("static fuzz path and binding must be valid");
        };
        let Ok(resolved) = path.resolve_against_with_query(&endpoint, Some(&query)) else {
            panic!("accepted declared query must remain inside a valid binding");
        };
        assert_eq!(resolved.query(), Some(query.as_str()));
        assert!(resolved.fragment().is_none());
    }

    // Declared quota headers: an accepted map names each field at most once, through a lowercase
    // header name that is neither credential-bearing nor framing.
    let entries = input
        .split('\0')
        .enumerate()
        .map(|(index, header)| (header, QUOTA_FIELDS[index % QUOTA_FIELDS.len()]));
    if let Ok(map) = ProviderQuotaHeaderMapV1::try_from_iter(entries) {
        assert!(map.iter().len() <= QUOTA_FIELDS.len());
        for (header, field) in map.iter() {
            assert!(header.len() <= MAX_QUOTA_HEADER_NAME_BYTES);
            assert!(
                header.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            );
            assert!(!PROVIDER_QUOTA_HEADER_DENIED_NAMES.contains(&header));
            assert_eq!(map.header_for(field), Some(header));
        }
    }
}

fuzz_target!(|data: &[u8]| {
    fuzz_eventstream(data);

    let Ok(input) = std::str::from_utf8(data) else {
        return;
    };

    // All Responses JSON facades consume untrusted bytes without ambient state.
    let context = south_north_codec::ResponsesContext {
        response_id: "resp_fuzz".to_owned(),
        model: "fuzz".to_owned(),
        created_at: 0,
        inbound_tools: Default::default(),
        reasoning: south_north_codec::ResponsesReasoningMode::Summary,
        allow_incomplete_tool_calls: false,
        render_legacy_encrypted_reasoning: false,
    };
    let _ = south_north_codec::responses::responses_request_json(input, &Default::default());
    let _ = south_north_codec::responses::responses_response_json(input, &context);
    let mut state = south_north_codec::ResponsesSseState::new(context);
    let _ = south_north_codec::responses::responses_event_json(input, &mut state);

    // The same public JSON ABI used by the MiniMax guest parses untrusted HTTP frames.
    let minimax = south_component_conformance::reference_minimax_task_v2::MiniMaxTaskReferenceV2;
    if let Ok(encoded) =
        south_component_conformance::abi_task_v2::parse_observation_json(&minimax, input)
    {
        let facts = parse_observation_json(&encoded).expect("guest output must decode");
        assert!(facts.validate().is_ok());
    }
    if let Ok(encoded) =
        south_component_conformance::abi_task_v2::parse_submit_response_json(&minimax, input)
    {
        assert!(parse_submit_outcome_json(&encoded).is_ok());
    }

    // Persisted task-v2 frames cross a separate trust boundary from provider JSON.
    // Accepted frames must survive the unique codec without losing typed facts.
    if let Ok(locator) = parse_locator_json(input) {
        assert_eq!(parse_locator_json(&locator_json(&locator).to_string()), Ok(locator));
    }
    if let Ok(observation) = parse_observation_json(input) {
        let encoded = observation_json(&observation).expect("decoded facts must encode");
        assert_eq!(parse_observation_json(&encoded.to_string()), Ok(observation));
    }
    if let Ok(prepared) = parse_prepared_task_json(input) {
        let estimate = &prepared.request_estimate;
        assert!(estimate.estimate_milliunits(f64::NAN).is_err());
        assert!(estimate.estimate_milliunits(-1.0).is_err());
        assert_eq!(
            estimate.estimate_milliunits(0.0).expect("zero host time is valid"),
            estimate.milliunits_per_second().map(|_| 0)
        );
        let encoded = prepared_task_json(&prepared).expect("decoded request must encode");
        assert_eq!(parse_prepared_task_json(&encoded.to_string()), Ok(prepared));
    }
    if let Ok(outcome) = parse_submit_outcome_json(input) {
        let encoded = submit_outcome_json(&outcome).expect("decoded outcome must encode");
        assert_eq!(parse_submit_outcome_json(&encoded.to_string()), Ok(outcome));
    }
    if let Ok(context) = parse_render_context_json(input) {
        assert_eq!(
            parse_render_context_json(&render_context_json(&context).to_string()),
            Ok(context)
        );
    }

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
        // An upstream may decode `%2F` before routing; read that way, an admitted path still has
        // no empty or dot segment (host feedback SF26, the kernel's D5 rule).
        let decoded = path.as_str().replace("%2F", "/").replace("%2f", "/");
        assert!(decoded.split('/').all(|segment| !matches!(segment, "" | "." | "..")));
        assert_eq!(RelativePathV1::parse(path.as_str()), Ok(path));
    }

    // Controlled query: a successfully constructed query must survive the join byte for byte,
    // against every valid binding, exactly like an accepted path must resolve.
    for parameter in QueryParameterV1::ALL {
        let Ok(query) = QueryStringV1::try_from_iter([(parameter.clone(), input)]) else {
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

    fuzz_declared_instances(input);

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
            present.iter().all(|(_, value)| value.len() <= MAX_PROVIDER_QUOTA_METADATA_VALUE_BYTES)
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

    let diagnostic_fields = input
        .split('\0')
        .enumerate()
        .map(|(index, value)| {
            (
                ResponseDiagnosticFieldV1::ALL[index % RESPONSE_DIAGNOSTIC_FIELD_COUNT],
                value.to_owned(),
            )
        })
        .collect::<Vec<_>>();
    if let Ok(diagnostics) = ResponseDiagnosticsV1::try_from_iter(diagnostic_fields) {
        let present = ResponseDiagnosticFieldV1::ALL
            .into_iter()
            .filter_map(|field| diagnostics.value(field).map(|value| (field, value)))
            .collect::<Vec<_>>();
        assert_eq!(present.len(), diagnostics.present_field_count());
        assert!(present.len() <= RESPONSE_DIAGNOSTIC_FIELD_COUNT);
        assert!(
            present.iter().all(|(_, value)| value.len() <= MAX_RESPONSE_DIAGNOSTIC_VALUE_BYTES)
        );
        assert!(
            present.iter().map(|(_, value)| value.len()).sum::<usize>()
                <= MAX_RESPONSE_DIAGNOSTIC_TOTAL_BYTES
        );
        // The accessor order and `iter()` must agree, or a host reading one and a person reading
        // the other would see different metadata for the same response.
        let iterated = diagnostics.iter().collect::<Vec<_>>();
        assert_eq!(iterated.len(), present.len());
        let rebuilt = ResponseDiagnosticsV1::try_from_iter(
            present.into_iter().map(|(field, value)| (field, value.to_owned())),
        );
        assert_eq!(rebuilt, Ok(diagnostics));
    }

    // The transcript is total: every input must produce a value that honours its own bounds and
    // never retains a denied header, whatever arbitrary bytes arrive.
    let transcript_headers = input
        .split('\0')
        .map(|chunk| chunk.split_once('=').unwrap_or((chunk, "")))
        .map(|(name, value)| (name, Some(value)))
        .collect::<Vec<_>>();
    let transcript = ResponseTranscriptV1::capture(transcript_headers);
    assert!(transcript.len() <= MAX_RESPONSE_TRANSCRIPT_COUNT);
    assert_eq!(transcript.is_empty(), transcript.len() == 0);
    assert!(
        transcript.iter().map(|(name, value)| name.len() + value.len()).sum::<usize>()
            <= MAX_RESPONSE_TRANSCRIPT_TOTAL_BYTES
    );
    for (name, value) in transcript.iter() {
        assert_eq!(name, name.to_ascii_lowercase());
        assert!(name.len() <= MAX_RESPONSE_TRANSCRIPT_NAME_BYTES);
        assert!(value.len() <= MAX_RESPONSE_TRANSCRIPT_VALUE_BYTES);
        // Whatever the upstream sent, a credential-bearing header never survives capture.
        assert!(!matches!(
            name,
            "authorization"
                | "proxy-authenticate"
                | "proxy-authorization"
                | "set-cookie"
                | "set-cookie2"
        ));
    }
});
