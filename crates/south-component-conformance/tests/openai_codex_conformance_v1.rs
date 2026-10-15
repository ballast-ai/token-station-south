//! Step R3 of the `OpenAI` Responses upstream record: the `openai-codex` family of
//! `provider-openai-responses` 1.2.0, its credential recipe, its fixture pack, and the stream form
//! of `UsageNeverDefaulted`.
//!
//! Design records: `docs/design/2026-09-30-openai-responses-upstream-component.md` (§3.3, §4.3,
//! §6.4, §10.2, §10.3, §12) and `docs/design/2026-10-12-responses-r3-codex-and-request-fields.md`
//! (§2, §3, §7).

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::reference_openai_responses::{
    CODEX_FAMILY, OpenAiResponsesReferenceV1,
};
use south_component_conformance::responses_vocabulary::{codex, extension};
use south_component_conformance::{
    CheckV1, ComponentResultV1, FixtureErrorV1, FixturePackV1, ProviderComponentV1,
    ProviderFamilyV1, StreamParserV1, VerdictV1, accepts_manifest,
    run_provider_component_suite_v1_for_manifest,
};
use south_provider_api::{ComponentManifestV1, ComponentMetadataV1};
use token_station_protocol::{
    ChatRequest, ChatResponse, ErrorCode, ErrorEnvelope, HttpRequestDescriptor, HttpResponseParts,
    ModelCapability, ProviderConfig, StreamEvent, Usage,
};

#[path = "support/openai_responses_packs.rs"]
mod packs;

fn manifest_value() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../components/provider-openai-responses/manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn shipped_manifest() -> ComponentManifestV1 {
    serde_json::from_value(manifest_value()).unwrap()
}

fn codex_pack() -> FixturePackV1 {
    packs::load(packs::CODEX_PACK)
}

fn input(name: &str) -> Value {
    let path = packs::pack_dir(packs::CODEX_PACK).join(format!("{name}.input.json"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn build(input: &Value) -> Result<HttpRequestDescriptor, ErrorEnvelope> {
    let config: ProviderConfig = serde_json::from_value(input["provider_config"].clone()).unwrap();
    let request: ChatRequest = serde_json::from_value(input["chat_request"].clone()).unwrap();
    OpenAiResponsesReferenceV1.build_http_request(&request, &config)
}

/// Every Codex request row that builds a request.
fn built_codex_rows() -> Vec<(String, Value)> {
    codex_pack()
        .cases()
        .iter()
        .filter(|case| {
            case.family == ProviderFamilyV1::Request && case.expected.get("error").is_none()
        })
        .map(|case| (case.name.clone(), case.input.clone()))
        .collect()
}

// -- the pack ------------------------------------------------------------------

/// R3 record §3.3: the rows the pack must carry. A pack can pass while having quietly lost a case.
#[test]
fn the_codex_pack_carries_every_decided_row() {
    let pack = codex_pack();
    let mut names: BTreeSet<String> = pack.cases().iter().map(|case| case.name.clone()).collect();
    names.extend(pack.credentials().cases().iter().map(|case| case.name.clone()));
    for row in [
        "provider.request.codex-stream",
        "provider.request.codex-without-account",
        "provider.request.codex-non-stream-refused",
        "provider.request.codex-store-never-forwarded",
        "provider.request.codex-request-fields-withheld",
        "provider.request.codex-tools-and-reasoning",
        "provider.stream.codex-host-pinned",
        "provider.stream.codex-text",
        "provider.stream.codex-tool-call",
        "provider.stream.codex-reasoning-encrypted-content",
        "provider.stream.codex-incomplete-max-output",
        "provider.stream.codex-failed",
        "provider.stream.codex-error-event",
        "provider.stream.codex-typeless-error-object",
        "provider.stream.codex-nonzero-tool-usage",
        "credential.rotation.codex-refresh",
        "credential.rotation.codex-rotation-without-new-token",
        "credential.rotation.codex-account-mismatch",
        "credential.clock.codex-fixed-window",
        "credential.clock.codex-use-stored",
        "credential.clock.codex-neither-token",
        "credential.on-status.codex-invalid-grant",
        "credential.on-status.codex-revoked",
        "credential.on-status.codex-rate-limited",
        "credential.on-status.codex-outage",
    ] {
        assert!(names.contains(row), "the Codex pack lost `{row}`");
    }
}

/// The directories are judged as one pack, so every Codex row is `codex-*`, every Codex
/// request row is for `openai-codex`, and no case name repeats across the two.
#[test]
fn the_codex_pack_holds_codex_rows_only_and_no_name_repeats() {
    let pack = codex_pack();
    for case in pack.cases() {
        let case_part = case.name.splitn(3, '.').nth(2).unwrap();
        assert!(case_part.starts_with("codex-"), "{}", case.name);
        if case.family == ProviderFamilyV1::Request {
            assert_eq!(case.input["provider_config"]["provider"], CODEX_FAMILY, "{}", case.name);
        }
    }
    for case in pack.credentials().cases() {
        assert!(case.name.splitn(3, '.').nth(2).unwrap().starts_with("codex-"), "{}", case.name);
    }
    let responses = packs::load(packs::RESPONSES_PACK);
    assert!(responses.cases().iter().all(|case| {
        case.family != ProviderFamilyV1::Request
            || case.input["provider_config"]["provider"] == "openai-responses"
    }));
    assert!(responses.credentials().cases().is_empty(), "credential rows live in the Codex pack");
    let union = packs::package_pack();
    let mut seen = BTreeSet::new();
    for name in union.cases().iter().map(|case| &case.name) {
        assert!(seen.insert(name.clone()), "`{name}` repeats across the directories");
    }
    let mantle = packs::load(packs::MANTLE_PACK);
    assert_eq!(
        union.cases().len(),
        responses.cases().len() + pack.cases().len() + mantle.cases().len()
    );
}

/// Record §12.2, moved here by §17.4: `UsageNeverDefaulted` runs on stream fixtures, and every
/// Codex stream row that reports usage carries the sidecar that lets it run.
#[test]
fn every_codex_stream_row_that_reports_usage_runs_usage_never_defaulted() {
    let report = run_provider_component_suite_v1_for_manifest(
        &OpenAiResponsesReferenceV1,
        &packs::package_pack(),
        &shipped_manifest(),
    );
    let mut judged = 0;
    for case in codex_pack().cases() {
        if case.family != ProviderFamilyV1::Stream {
            continue;
        }
        let reports_usage = case
            .expected
            .as_array()
            .is_some_and(|events| events.iter().any(|event| event["type"] == "usage"));
        if !reports_usage {
            assert!(case.usage_chunk.is_none(), "{}", case.name);
            continue;
        }
        assert!(case.usage_chunk.is_some(), "{} carries no stream usage sidecar", case.name);
        assert!(
            report.outcomes().iter().any(|outcome| outcome.check == CheckV1::UsageNeverDefaulted
                && outcome.case == case.name
                && outcome.verdict == VerdictV1::Passed),
            "{}: {report}",
            case.name
        );
        judged += 1;
    }
    assert!(judged >= 5, "too few stream rows judged");
}

/// A Responses component that turns a terminal frame without usage into a zero report: the
/// pre-B1 fault, on the stream path only.
struct ZeroDefaulting;

struct ZeroDefaultingParser(Box<dyn StreamParserV1>);

impl StreamParserV1 for ZeroDefaultingParser {
    fn parse_chunk(&mut self, chunk: &[u8]) -> ComponentResultV1<Vec<StreamEvent>> {
        match self.0.parse_chunk(chunk) {
            Err(error) if error.code == ErrorCode::ProviderProtocolError => Ok(vec![
                StreamEvent::Usage { usage: Usage::default() },
                StreamEvent::Done { finish_reason: None, stop_sequence: None },
            ]),
            other => other,
        }
    }
}

impl ProviderComponentV1 for ZeroDefaulting {
    fn metadata(&self) -> ComponentMetadataV1 {
        OpenAiResponsesReferenceV1.metadata()
    }
    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<ModelCapability>> {
        OpenAiResponsesReferenceV1.model_capabilities(config)
    }
    fn build_http_request(
        &self,
        request: &ChatRequest,
        config: &ProviderConfig,
    ) -> ComponentResultV1<HttpRequestDescriptor> {
        OpenAiResponsesReferenceV1.build_http_request(request, config)
    }
    fn parse_response(&self, parts: &HttpResponseParts) -> ComponentResultV1<ChatResponse> {
        OpenAiResponsesReferenceV1.parse_response(parts)
    }
    fn map_provider_error(&self, parts: &HttpResponseParts) -> ComponentResultV1<ErrorEnvelope> {
        OpenAiResponsesReferenceV1.map_provider_error(parts)
    }
    fn stream_parser(&self) -> Box<dyn StreamParserV1> {
        Box::new(ZeroDefaultingParser(OpenAiResponsesReferenceV1.stream_parser()))
    }
}

/// The stream check turns red on a component that reports zeros for a missing stream usage, and
/// on nothing else: every Codex stream row with a sidecar fails it, no other check's verdict on
/// those rows depends on it.
#[test]
fn the_stream_usage_check_bites_on_a_component_that_defaults_missing_usage() {
    let pack = codex_pack();
    let report = run_provider_component_suite_v1_for_manifest(
        &ZeroDefaulting,
        &FixturePackV1::from_cases(pack.cases().to_vec()),
        &shipped_manifest(),
    );
    let red: BTreeSet<&str> = report
        .failures()
        .filter(|outcome| outcome.check == CheckV1::UsageNeverDefaulted)
        .map(|outcome| outcome.case.as_str())
        .collect();
    let expected: BTreeSet<&str> = pack
        .cases()
        .iter()
        .filter(|case| case.usage_chunk.is_some())
        .map(|case| case.name.as_str())
        .collect();
    assert!(!expected.is_empty());
    assert_eq!(red, expected, "{report}");
}

/// A stream sidecar names its chunk and its pointer, exactly; anything else refuses the pack.
#[test]
fn the_stream_usage_sidecar_is_strict() {
    let directory =
        std::env::temp_dir().join(format!("south-codex-stream-meta-{}", std::process::id()));
    for meta in [
        r#"{"usage_pointer": "/response/usage"}"#,
        r#"{"usage_chunk": 0}"#,
        r#"{"usage_chunk": -1, "usage_pointer": "/response/usage"}"#,
        r#"{"usage_chunk": "0", "usage_pointer": "/response/usage"}"#,
        r#"{"usage_chunk": 0, "usage_pointer": ""}"#,
        r#"{"usage_chunk": 0, "usage_pointer": "/response/usage", "extra": 1}"#,
    ] {
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        for (suffix, body) in [("input", "{}"), ("expected", "[]"), ("meta", meta)] {
            std::fs::write(directory.join(format!("provider.stream.a.{suffix}.json")), body)
                .unwrap();
        }
        let error = FixturePackV1::load(&directory).expect_err(meta);
        assert!(matches!(error, FixtureErrorV1::InvalidMeta { .. }), "{meta}: {error}");
    }
    std::fs::remove_dir_all(&directory).ok();
}

/// A sidecar that names nothing is a failed check, not a pass.
#[test]
fn a_stream_sidecar_that_names_nothing_fails_the_check() {
    let pack = codex_pack();
    let case = pack.cases().iter().find(|case| case.name == "provider.stream.codex-text").unwrap();
    let last = case.input["chunks"].as_array().unwrap().len();
    for (chunk, pointer) in [(last, "/response/usage"), (0, "/response/usage"), (last - 1, "/nope")]
    {
        let mut broken = case.clone();
        broken.usage_chunk = Some(chunk);
        broken.usage_pointer = Some(pointer.to_owned());
        let report = run_provider_component_suite_v1_for_manifest(
            &OpenAiResponsesReferenceV1,
            &FixturePackV1::from_cases(vec![broken]),
            &shipped_manifest(),
        );
        assert!(
            report.failures().any(|outcome| outcome.check == CheckV1::UsageNeverDefaulted),
            "chunk {chunk}, `{pointer}`: {report}"
        );
    }
}

// -- gate ① ------------------------------------------------------------------------

/// R3 record §2.1, §7: gate ① admits the shipped section, and refuses the three shapes the record
/// rejected — the parent record's two-slot sketch, a file-keyed `import`, and `account_id` without
/// a value syntax.
#[test]
fn gate_one_admits_the_section_and_refuses_the_rejected_shapes() {
    assert_eq!(accepts_manifest(&shipped_manifest()), Ok(()));

    let refused = |mutate: &dyn Fn(&mut Value)| {
        let mut wire = manifest_value();
        mutate(&mut wire);
        serde_json::from_value::<ComponentManifestV1>(wire)
            .map_or(true, |manifest| accepts_manifest(&manifest).is_err())
    };
    assert!(refused(&|wire| {
        wire["permissions"]["secrets"] = json!(["provider_api_key", "codex_access_token"]);
        wire["credentials"]["slots"] = json!({"codex_access_token": {"minted": "codex"}});
    }));
    assert!(refused(&|wire| {
        wire["credentials"]["import"] = json!({"codex_auth_json": {
            "access_token": {"pointers": ["/tokens/access_token"]},
            "refresh_token": {"pointers": ["/tokens/refresh_token"]},
            "account_id": {"pointers": ["/tokens/account_id"]}}});
    }));
    assert!(refused(&|wire| {
        wire["credentials"]["fields"]["account_id"].as_object_mut().unwrap().remove("syntax");
    }));
}

/// The test recipe `codex()` of `credential_recipe_v1`, as it was before R3 promoted it.
fn test_recipe() -> Value {
    json!({
        "schema": "south.credential-recipe.v1",
        "fields": {
            "access_token": { "secret": true },
            "refresh_token": { "secret": true },
            "account_id": { "secret": false, "syntax": { "printable_ascii": 128 } }
        },
        "require_one_of": [["access_token", "refresh_token"]],
        "slots": { "provider_api_key": { "minted": "codex" } },
        "recipes": {
            "codex": {
                "steps": [
                    { "id": "refresh", "kind": "oauth2_token", "encoding": "json",
                      "endpoint": "https://auth.openai.com/oauth/token",
                      "requires": ["refresh_token"],
                      "params": {
                          "grant_type": { "const": "refresh_token" },
                          "refresh_token": { "field": "refresh_token" },
                          "client_id": { "const": "app_EMoamEEZ73f0CkXaXp7hrann" } },
                      "extract": {
                          "access_token": { "pointer": "/access_token", "secret": true },
                          "refresh_token": { "pointer": "/refresh_token", "secret": true, "optional": true },
                          "account": {
                              "jwt_claim": { "token": "/id_token",
                                             "pointer": "/https:~1~1api.openai.com~1auth/chatgpt_account_id" },
                              "optional": true, "must_equal_field": "account_id" },
                          "expires_at": { "fixed_window": true } } }
                ],
                "present": "refresh.access_token",
                "rotates_refresh_material": true,
                "write_back": { "refresh_token": "refresh.refresh_token" },
                "without_refresh_material": "use_stored",
                "fixed_validity_seconds": 3000,
                "attributes": { "account_id": { "field": "account_id", "export": true, "persist": true } }
            }
        }
    })
}

/// R3 record §2.2: the production recipe is the test recipe with exactly the R3 changes — `scope`
/// sent, `import` from the Codex CLI's `auth.json`, `account_id` of syntax `token`, and a 429 from the
/// token endpoint `transient` (§9 item 8, RQ3-Q9) — scoped to the Codex family, plus the operator-form descriptions, which nothing interprets.
#[test]
fn the_recipe_is_the_test_recipe_with_the_r3_changes() {
    let mut expected = test_recipe();
    expected["families"] = json!([CODEX_FAMILY]);
    expected["recipes"]["codex"]["steps"][0]["params"]["scope"] =
        json!({"const": "openid profile email offline_access"});
    let import = |pointer: &str| json!({"file": "codex-auth-json", "pointers": [pointer]});
    expected["import"] = json!({
        "access_token": import("/tokens/access_token"),
        "refresh_token": import("/tokens/refresh_token"),
        "account_id": import("/tokens/account_id"),
    });
    expected["fields"]["account_id"]["syntax"] = json!("token");
    // RQ3-Q9 (lv, 2026-10-12): a rate limit on the token endpoint is transient, as for Copilot.
    expected["recipes"]["codex"]["steps"][0]["on_status"] = json!({"429": "transient"});

    let mut shipped = manifest_value()["credentials"].clone();
    for field in shipped["fields"].as_object_mut().unwrap().values_mut() {
        assert!(field["description"].is_string(), "every field tells the operator what it is");
        field.as_object_mut().unwrap().remove("description");
    }
    let parse = |value: Value| -> south_provider_api::CredentialsV1 {
        serde_json::from_value(value).unwrap()
    };
    assert_eq!(parse(shipped), parse(expected));
    // The section applies to Codex only: `openai-responses` keeps its static key.
    let manifest = shipped_manifest();
    assert!(manifest.credentials_for(CODEX_FAMILY).is_some());
    assert!(manifest.credentials_for("openai-responses").is_none());
    assert_eq!(manifest.declared_keys(CODEX_FAMILY), BTreeSet::from([codex::ACCOUNT_ATTRIBUTE]));
    assert!(manifest.declared_keys("openai-responses").is_empty());
}

// -- requests ------------------------------------------------------------------------

/// Record §4.3: whatever the IR carries, a Codex body has no `temperature`, no
/// `max_output_tokens` and none of the four client fields; it always has `store: false`,
/// `stream: true` and `instructions`; and it goes to `{base_url}/codex/responses` with
/// `accept: text/event-stream`.
#[test]
fn codex_requests_never_carry_what_the_family_removes() {
    let rows = built_codex_rows();
    assert!(rows.len() >= 5);
    for (name, mut input) in rows {
        let request = &mut input["chat_request"];
        request["sampling"]["temperature"] = json!(1.3);
        request["sampling"]["max_output_tokens"] = json!(4096);
        for (key, value) in [
            ("store", json!(true)),
            ("temperature", json!(0.2)),
            ("max_output_tokens", json!(64)),
            (extension::USER, json!("user-42")),
            (extension::METADATA, json!({"tenant": "acme"})),
            (extension::PROMPT_CACHE_KEY, json!("k")),
            (extension::TRUNCATION, json!("auto")),
        ] {
            request[key] = value;
        }
        let descriptor = build(&input).unwrap_or_else(|error| panic!("{name}: {error:?}"));
        let wire = serde_json::to_value(&descriptor).unwrap();
        let body = &wire["body"];
        for absent in [
            "temperature",
            "max_output_tokens",
            "user",
            "metadata",
            "prompt_cache_key",
            "truncation",
        ] {
            assert!(body.get(absent).is_none(), "{name}: `{absent}` reached a Codex body");
        }
        assert_eq!(body["store"], json!(false), "{name}");
        assert_eq!(body["stream"], json!(true), "{name}");
        assert!(body["instructions"].is_string(), "{name}");
        assert!(body["input"].is_array(), "{name}");
        assert_eq!(wire["url"], "https://chatgpt.com/backend-api/codex/responses", "{name}");
        assert_eq!(wire["headers"]["accept"], "text/event-stream", "{name}");
    }
}

/// R-Q5: a non-streaming Codex request is a capability error before any request exists, so no
/// upstream call can follow; every Codex row with `stream` turned off is refused the same way.
#[test]
fn a_non_streaming_codex_request_is_refused_at_build_time() {
    for (name, mut input) in built_codex_rows() {
        input["chat_request"]["stream"] = json!(false);
        let refused = build(&input).expect_err(&name);
        assert_eq!(refused.code, ErrorCode::Capability, "{name}");
        assert_eq!(refused.http_status, 400, "{name}");
        input["chat_request"].as_object_mut().unwrap().remove("stream");
        assert_eq!(build(&input).unwrap_err().code, ErrorCode::Capability, "{name}");
    }
}

/// Record §10.3: `chatgpt-account-id` comes from the credential's exported attribute in
/// `ProviderConfig.declared` and from nothing else — not from `extensions`, not from
/// `host_values` — and is absent when the host passes no attribute.
#[test]
fn the_account_header_comes_from_the_declared_attribute_only() {
    let with = input("provider.request.codex-stream");
    assert_eq!(with["provider_config"]["declared"][codex::ACCOUNT_ATTRIBUTE], "acct-1");
    let header = |input: &Value| {
        serde_json::to_value(build(input).unwrap()).unwrap()["headers"]
            .get(codex::ACCOUNT_HEADER)
            .cloned()
    };
    assert_eq!(header(&with), Some(json!("acct-1")));

    let mut without = with.clone();
    without["provider_config"].as_object_mut().unwrap().remove("declared");
    assert_eq!(header(&without), None);
    for (place, key) in [
        ("/chat_request", codex::ACCOUNT_ATTRIBUTE),
        ("/chat_request", codex::ACCOUNT_HEADER),
        ("/provider_config", codex::ACCOUNT_ATTRIBUTE),
        ("/provider_config", codex::ACCOUNT_HEADER),
    ] {
        let mut smuggled = without.clone();
        smuggled.pointer_mut(place).unwrap()[key] = json!("acct-other");
        assert_eq!(header(&smuggled), None, "{place}/{key}");
    }
    let mut host_value = without.clone();
    host_value["chat_request"]["host_values"] = json!({codex::ACCOUNT_ATTRIBUTE: "acct-other"});
    assert_eq!(header(&host_value), None);
    // The `openai-responses` family never sends it, attribute or not.
    let mut other = with;
    other["provider_config"]["provider"] = json!("openai-responses");
    assert_eq!(header(&other), None);
}

/// Record §4.3: a request's own instructions win; the default fills only their absence.
#[test]
fn the_default_instructions_fill_only_an_absent_instruction() {
    let body =
        |name: &str| serde_json::to_value(build(&input(name)).unwrap()).unwrap()["body"].clone();
    assert_eq!(body("provider.request.codex-stream")["instructions"], codex::DEFAULT_INSTRUCTIONS);
    assert_eq!(body("provider.request.codex-without-account")["instructions"], "You are terse.");
}

// -- credentials ---------------------------------------------------------------------

/// R3 record §7: the credential rows show what the test plan lists — refresh with `scope`,
/// rotation and write-back, the fixed window, the `must_equal_field` conflict, missing refresh
/// material, 4xx `reauth_required`, 5xx transient — and gate ② ran every one of them.
#[test]
fn the_credential_rows_show_the_r3_test_plan() {
    let pack = codex_pack();
    let expected = |name: &str| {
        pack.credentials()
            .cases()
            .iter()
            .find(|case| case.name == format!("credential.{name}"))
            .unwrap_or_else(|| panic!("{name}"))
            .expected
            .clone()
    };
    let refresh = expected("rotation.codex-refresh");
    let body: Value =
        serde_json::from_str(refresh["requests"][0]["body"].as_str().unwrap()).unwrap();
    assert_eq!(body["scope"], "openid profile email offline_access");
    assert_eq!(body["client_id"], "app_EMoamEEZ73f0CkXaXp7hrann");
    let minted = &refresh["outcome"]["minted"];
    assert_eq!(minted["write_back"], json!({"refresh_token": "fake-refresh-2"}));
    assert_eq!(minted["attributes"], json!({"account_id": "acct-1"}));
    let now = 1_767_225_600_i64;
    assert_eq!(minted["expires_at"], json!(now + 3000), "expires_in is ignored");
    assert_eq!(
        expected("clock.codex-fixed-window")["outcome"]["minted"]["expires_at"],
        json!(now + 3000)
    );
    assert_eq!(
        expected("rotation.codex-rotation-without-new-token")["outcome"]["minted"]["write_back"],
        json!({})
    );
    assert_eq!(
        expected("rotation.codex-account-mismatch")["outcome"],
        json!({"reauth_required": {"step": "refresh", "status": 200}})
    );
    assert_eq!(expected("clock.codex-use-stored")["outcome"], json!("use_stored"));
    for (name, outcome) in [
        ("on-status.codex-invalid-grant", "reauth_required"),
        ("on-status.codex-revoked", "reauth_required"),
        ("on-status.codex-rate-limited", "transient"),
        ("on-status.codex-outage", "transient"),
    ] {
        assert!(expected(name)["outcome"].get(outcome).is_some(), "{name}");
    }

    let report = run_provider_component_suite_v1_for_manifest(
        &OpenAiResponsesReferenceV1,
        &packs::package_pack(),
        &shipped_manifest(),
    );
    for case in pack.credentials().cases() {
        assert!(
            report.outcomes().iter().any(|outcome| outcome.check == CheckV1::CredentialRecipeMatch
                && outcome.case == case.name
                && outcome.verdict == VerdictV1::Passed),
            "{}: {report}",
            case.name
        );
    }
}

/// Without the Codex directory the package does not pass gate ②: the manifest's recipe owes its
/// credential rows (and its immutable Codex paths a built request), which only that directory
/// carries.
#[test]
fn the_responses_directory_alone_does_not_pass_gate_two() {
    let report = run_provider_component_suite_v1_for_manifest(
        &OpenAiResponsesReferenceV1,
        &packs::load(packs::RESPONSES_PACK),
        &shipped_manifest(),
    );
    let red: BTreeSet<CheckV1> = report.failures().map(|outcome| outcome.check).collect();
    assert!(red.contains(&CheckV1::Coverage), "{report}");
    assert!(red.contains(&CheckV1::ImmutablePathsHonoured), "{report}");
}
