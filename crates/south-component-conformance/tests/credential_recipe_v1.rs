//! The reference credential-recipe interpreter and gate ②'s `CredentialRecipeMatch` (B4,
//! `docs/design/2026-09-30-host-zero-vendor-boundary.md` §3.3, §3.5, §3.7).
//!
//! Three things are proved here: the Kling recipe builds byte for byte the JWT the host builds
//! today; the interpreter means what §3.3 says for every form the five families use; and the new
//! check and its coverage rule turn red on a wrong recipe or a wrong fixture.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::credential_recipe::{
    FakeResponseV1, FixtureSignerV1, JwtSignerV1, RecipeEffectsV1, RecipeOutcomeV1, RecipeRunV1,
    RenderedRequestV1, run_recipe_v1,
};
use south_component_conformance::reference::OpenAiCompatibleReferenceV1;
use south_component_conformance::{
    CheckV1, CredentialCaseV1, CredentialFamilyV1, CredentialFixturePackV1, FixturePackV1,
    OutcomeV1, credential_recipe_checks_v1, run_provider_component_suite_v1_for_manifest,
};
use south_provider_api::{ComponentManifestV1, CredentialsV1, JwtAlgorithmV1};

const NOW: i64 = 1_767_225_600;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn shipped(package: &str) -> ComponentManifestV1 {
    let path = root().join(format!("../../components/{package}/manifest.json"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// A shipped provider manifest carrying `credentials`, through gate ①.
fn manifest_with(credentials: &Value) -> ComponentManifestV1 {
    let mut manifest = serde_json::to_value(shipped("provider-openai-compatible")).unwrap();
    manifest["credentials"] = credentials.clone();
    let manifest: ComponentManifestV1 = serde_json::from_value(manifest).unwrap();
    manifest.validate().expect("the recipe passes gate ①");
    manifest
}

fn credentials(value: &Value) -> CredentialsV1 {
    manifest_with(value).credentials.unwrap()
}

/// Runs `slot` with the fixture signer and `responses` keyed by step id.
fn run_at(
    credentials: &CredentialsV1,
    now: i64,
    fields: &[(&str, &str)],
    responses: &Value,
) -> RecipeRunV1 {
    let fields: BTreeMap<String, String> =
        fields.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect();
    let responses: BTreeMap<String, FakeResponseV1> =
        serde_json::from_value(responses.clone()).unwrap();
    let mut responder = |request: &RenderedRequestV1| responses.get(&request.step).cloned();
    run_recipe_v1(
        credentials,
        "provider_api_key",
        &fields,
        &mut RecipeEffectsV1 { now, signer: &FixtureSignerV1, responder: &mut responder },
    )
}

fn mint_once(
    credentials: &CredentialsV1,
    fields: &[(&str, &str)],
    responses: &Value,
) -> RecipeRunV1 {
    run_at(credentials, NOW, fields, responses)
}

fn minted(run: &RecipeRunV1) -> &south_component_conformance::credential_recipe::MintedV1 {
    match &run.outcome {
        RecipeOutcomeV1::Minted(minted) => minted,
        other => panic!("expected a minted value, got {other:?}"),
    }
}

/// Base64url without padding, written independently of the interpreter's.
fn b64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let mut padded = [0u8; 3];
        padded[..chunk.len()].copy_from_slice(chunk);
        let n = (u32::from(padded[0]) << 16) | (u32::from(padded[1]) << 8) | u32::from(padded[2]);
        for index in 0..=chunk.len() {
            out.push(char::from(ALPHABET[((n >> (18 - 6 * index)) & 63) as usize]));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// The golden vector: the Kling recipe against the host's minting.
// ---------------------------------------------------------------------------------------------

/// One signing request: the algorithm, the key bytes and the signing input.
type SignCall = (JwtAlgorithmV1, Vec<u8>, Vec<u8>);

/// Hands back fixed signature bytes and records what it was asked to sign.
struct HostBytes {
    signature: Vec<u8>,
    seen: RefCell<Vec<SignCall>>,
}

impl JwtSignerV1 for HostBytes {
    fn sign(&self, alg: JwtAlgorithmV1, key: &[u8], input: &[u8]) -> Result<Vec<u8>, String> {
        self.seen.borrow_mut().push((alg, key.to_vec(), input.to_vec()));
        Ok(self.signature.clone())
    }
}

/// The host mints Kling's bearer in `generate_kling_jwt`
/// (server `gateway/src/modules/inference/engine/upstream.rs`): `jsonwebtoken` 9.3.1,
/// `Header::new(Algorithm::HS256)` and a `Claims { iss, exp: now + 1800, nbf: now - 5 }` struct,
/// keyed with `EncodingKey::from_secret(secret_key.as_bytes())`. That code, run with `now` fixed at
/// [`NOW`], access key `fake-access-key` and secret key `fake-secret-key`, produced the token
/// below; its signature is the real HMAC-SHA256 of the signing input under that key.
///
/// So `jsonwebtoken` serializes the header as `{"typ":"JWT","alg":"HS256"}` — `typ` first — and
/// the claims in struct order `iss`, `exp`, `nbf`. The interpreter matches both: its header puts
/// `typ` first, and its claim order (RFC 7519's registered claims first, in RFC order) yields
/// `iss`, `exp`, `nbf`.
const HOST_KLING_JWT: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.\
                              eyJpc3MiOiJmYWtlLWFjY2Vzcy1rZXkiLCJleHAiOjE3NjcyMjc0MDAsIm5iZiI6MTc2NzIyNTU5NX0.\
                              _OPkl4SrKHhQgw1EsI2zIy0ZwoR10lDdV-dUp7NhsbI";

/// The host token's signature bytes (HMAC-SHA256 under `fake-secret-key`).
const HOST_KLING_SIGNATURE: [u8; 32] = [
    252, 227, 228, 151, 132, 171, 40, 120, 80, 131, 13, 68, 176, 141, 179, 35, 45, 25, 194, 132,
    117, 210, 80, 221, 87, 231, 84, 167, 179, 97, 177, 178,
];

#[test]
fn the_kling_recipe_builds_the_jwt_the_host_builds_byte_for_byte() {
    let credentials = shipped("task-kling-v2").credentials.expect("Kling declares its recipe");
    let signer = HostBytes { signature: HOST_KLING_SIGNATURE.to_vec(), seen: RefCell::default() };
    let fields: BTreeMap<String, String> = [
        ("access_key".to_owned(), "fake-access-key".to_owned()),
        ("secret_key".to_owned(), "fake-secret-key".to_owned()),
    ]
    .into();
    let mut no_exchange = |_: &RenderedRequestV1| -> Option<FakeResponseV1> {
        panic!("a jwt_sign recipe makes no request")
    };
    let run = run_recipe_v1(
        &credentials,
        "provider_api_key",
        &fields,
        &mut RecipeEffectsV1 { now: NOW, signer: &signer, responder: &mut no_exchange },
    );

    let header = br#"{"typ":"JWT","alg":"HS256"}"#;
    let claims = br#"{"iss":"fake-access-key","exp":1767227400,"nbf":1767225595}"#;
    let signing_input = format!("{}.{}", b64(header), b64(claims));
    assert_eq!(
        format!("{signing_input}.{}", b64(&HOST_KLING_SIGNATURE)),
        HOST_KLING_JWT,
        "the literal header and claims are what the host serialized"
    );
    assert_eq!(minted(&run).present, HOST_KLING_JWT);
    assert_eq!(minted(&run).expires_at, NOW + 1800, "the JWT's own validity window");
    assert!(run.requests.is_empty());
    assert_eq!(
        *signer.seen.borrow(),
        [(JwtAlgorithmV1::HS256, b"fake-secret-key".to_vec(), signing_input.into_bytes())],
        "the key is the secret key's bytes and the signer sees exactly the host's signing input"
    );
}

// ---------------------------------------------------------------------------------------------
// The interpreter, form by form, on the five families of §3.9.
// ---------------------------------------------------------------------------------------------

fn codex() -> Value {
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

/// An unsigned JWT whose payload names account `acct-1`.
const ID_TOKEN_ACCT_1: &str = "eyJhbGciOiJub25lIn0.\
     eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjdC0xIn19.sig";
/// The same, naming `acct-other`.
const ID_TOKEN_OTHER: &str = "eyJhbGciOiJub25lIn0.\
     eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjdC1vdGhlciJ9fQ.sig";

const CODEX_FIELDS: [(&str, &str); 2] =
    [("refresh_token", "fake-refresh-1"), ("account_id", "acct-1")];

fn codex_rotation_response() -> Value {
    json!({ "refresh": { "status": 200, "body": {
        "access_token": "fake-access-2", "refresh_token": "fake-refresh-2",
        "id_token": ID_TOKEN_ACCT_1, "expires_in": 7200 } } })
}

/// The expected run of [`codex_rotation_response`], written by hand from §3.3 and §3.9.
fn codex_rotation_expected() -> Value {
    json!({
        "recipe": "codex",
        "requests": [{
            "step": "refresh",
            "method": "POST",
            "url": "https://auth.openai.com/oauth/token",
            "headers": { "content-type": "application/json" },
            "body": concat!(
                r#"{"client_id":"app_EMoamEEZ73f0CkXaXp7hrann","grant_type":"refresh_token","#,
                r#""refresh_token":"fake-refresh-1"}"#
            )
        }],
        "outcome": { "minted": {
            "present": "fake-access-2",
            // fixed_window ignores expires_in: 3000 s after now.
            "expires_at": NOW + 3000,
            "outputs": {
                "refresh.access_token": "fake-access-2",
                "refresh.account": "acct-1",
                "refresh.expires_at": NOW + 3000,
                "refresh.refresh_token": "fake-refresh-2"
            },
            "write_back": { "refresh_token": "fake-refresh-2" },
            "attributes": { "account_id": "acct-1" }
        } }
    })
}

#[test]
fn a_rotating_refresh_writes_back_the_new_material_and_exports_only_fields() {
    let codex_recipe = credentials(&codex());
    let run = mint_once(&codex_recipe, &CODEX_FIELDS, &codex_rotation_response());
    assert_eq!(serde_json::to_value(&run).unwrap(), codex_rotation_expected());
}

#[test]
fn rotation_that_returns_no_material_keeps_the_stored_value() {
    let codex_recipe = credentials(&codex());
    // Absent, empty and null refresh tokens all keep what is stored (§3.5 no-wipe).
    for refresh_token in [None, Some(json!("")), Some(Value::Null)] {
        let mut body = json!({ "access_token": "fake-access-2", "id_token": ID_TOKEN_ACCT_1 });
        if let Some(refresh_token) = refresh_token {
            body["refresh_token"] = refresh_token;
        }
        let run = mint_once(
            &codex_recipe,
            &CODEX_FIELDS,
            &json!({ "refresh": { "status": 200, "body": body } }),
        );
        assert_eq!(minted(&run).present, "fake-access-2");
        assert!(minted(&run).write_back.is_empty(), "{run:?}");
    }
}

#[test]
fn a_claim_naming_another_account_is_reauth_and_writes_nothing_back() {
    let codex_recipe = credentials(&codex());
    let run = mint_once(
        &codex_recipe,
        &CODEX_FIELDS,
        &json!({ "refresh": { "status": 200, "body": {
            "access_token": "fake-access-2", "refresh_token": "fake-refresh-2",
            "id_token": ID_TOKEN_OTHER } } }),
    );
    assert_eq!(
        serde_json::to_value(&run.outcome).unwrap(),
        json!({ "reauth_required": { "step": "refresh", "status": 200 } })
    );
    // With no stored account to compare against, the claim is no obstacle.
    let run = run_at(
        &codex_recipe,
        NOW,
        &[("refresh_token", "fake-refresh-1")],
        &json!({ "refresh": { "status": 200, "body": {
            "access_token": "fake-access-2", "id_token": ID_TOKEN_OTHER } } }),
    );
    assert_eq!(minted(&run).outputs["refresh.account"], "acct-other");
    assert!(minted(&run).attributes.is_empty());
}

#[test]
fn missing_refresh_material_uses_the_stored_value_or_is_a_configuration_error() {
    let codex_recipe = credentials(&codex());
    let run = mint_once(&codex_recipe, &[("access_token", "fake-access-1")], &json!({}));
    assert_eq!(run.outcome, RecipeOutcomeV1::UseStored);
    assert!(run.requests.is_empty());

    let mut failing = codex();
    failing["recipes"]["codex"]["without_refresh_material"] = json!("fail");
    let run = mint_once(&credentials(&failing), &[("access_token", "fake-access-1")], &json!({}));
    assert_eq!(
        serde_json::to_value(&run.outcome).unwrap(),
        json!({ "configuration": { "field": "refresh_token" } })
    );
    assert!(run.requests.is_empty(), "no request without what the step requires");

    // Neither field of the require_one_of group: the stored credential is unusable.
    let run = mint_once(&codex_recipe, &[("account_id", "acct-1")], &json!({}));
    assert!(matches!(run.outcome, RecipeOutcomeV1::Configuration { .. }), "{run:?}");
    assert_eq!(run.recipe, None);
}

#[test]
fn a_required_extraction_missing_from_the_response_is_transient() {
    let codex_recipe = credentials(&codex());
    let run = mint_once(
        &codex_recipe,
        &CODEX_FIELDS,
        &json!({ "refresh": { "status": 200, "body": {} } }),
    );
    assert_eq!(
        serde_json::to_value(&run.outcome).unwrap(),
        json!({ "transient": { "step": "refresh", "status": 200 } })
    );
    // No response at all (a transport failure) is transient too.
    let run = mint_once(&codex_recipe, &CODEX_FIELDS, &json!({}));
    assert_eq!(
        serde_json::to_value(&run.outcome).unwrap(),
        json!({ "transient": { "step": "refresh" } })
    );
}

fn kiro() -> Value {
    let refresh = |id: &str, endpoint: &str, params: Value| {
        json!({
            "steps": [
                { "id": id, "kind": "http_exchange", "method": "POST", "encoding": "json",
                  "endpoint": endpoint,
                  "endpoint_params": { "region": { "field": "region" } },
                  "requires": ["refresh_token"],
                  "params": params,
                  "on_status": { "400": "reauth_required", "401": "reauth_required", "4xx": "transient" },
                  "extract": {
                      "access_token": { "pointer": "/accessToken", "secret": true },
                      "refresh_token": { "pointer": "/refreshToken", "secret": true, "optional": true },
                      "expires_at": { "relative_seconds": "/expiresIn" } } }
            ],
            "present": format!("{id}.access_token"),
            "rotates_refresh_material": true,
            "write_back": { "refresh_token": format!("{id}.refresh_token") },
            "attributes": { "profile_arn": { "field": "profile_arn", "export": true, "persist": true } }
        })
    };
    json!({
        "schema": "south.credential-recipe.v1",
        "fields": {
            "refresh_token": { "secret": true, "required": true },
            "client_id": { "secret": false, "syntax": { "printable_ascii": 256 } },
            "client_secret": { "secret": true },
            "auth_method": { "secret": false, "syntax": { "enum": ["social", "idc"] }, "default": "social" },
            "region": { "secret": false, "syntax": "aws_region", "default": "us-east-1" },
            "profile_arn": { "secret": false, "syntax": "aws_arn" }
        },
        "slots": { "provider_api_key": { "minted": "kiro" } },
        "recipes": {
            "kiro": { "select": [
                { "when": { "field_in": { "field": "auth_method", "values": ["IDC"] } }, "recipe": "idc" },
                { "when": { "all_present": ["client_id", "client_secret"] }, "recipe": "idc" },
                { "recipe": "social" } ] },
            "social": refresh("social", "https://prod.{region}.auth.desktop.kiro.dev/refreshToken",
                              json!({ "refreshToken": { "field": "refresh_token" } })),
            "idc": refresh("idc", "https://oidc.{region}.amazonaws.com/token", json!({
                "refreshToken": { "field": "refresh_token" },
                "clientId": { "field": "client_id" },
                "clientSecret": { "field": "client_secret" },
                "grantType": { "const": "refresh_token" } }))
        }
    })
}

const ARN: &str = "arn:aws:codewhisperer:us-east-1:123456789012:profile/FAKE";

#[test]
fn a_selector_picks_the_first_matching_rule_and_the_template_takes_the_region() {
    let kiro_recipe = credentials(&kiro());
    let ok = json!({ "status": 200, "body": { "accessToken": "fake-kiro", "expiresIn": 3600 } });

    // Defaults fill the empty method and region; the social form sends no grant type.
    let run = mint_once(
        &kiro_recipe,
        &[("refresh_token", "fake-refresh"), ("profile_arn", ARN)],
        &json!({ "social": ok }),
    );
    assert_eq!(run.recipe.as_deref(), Some("social"));
    assert_eq!(
        serde_json::to_value(&run.requests).unwrap(),
        json!([{ "step": "social", "method": "POST",
                 "url": "https://prod.us-east-1.auth.desktop.kiro.dev/refreshToken",
                 "headers": { "content-type": "application/json" },
                 "body": "{\"refreshToken\":\"fake-refresh\"}" }])
    );
    assert_eq!(minted(&run).expires_at, NOW + 3600);
    assert_eq!(minted(&run).attributes, BTreeMap::from([("profile_arn".into(), ARN.into())]));
    assert!(
        minted(&run).write_back.is_empty(),
        "no refreshToken in the response keeps the stored one"
    );

    // `field_in` compares ASCII case-insensitively: the rule lists `IDC`, the field holds `idc`.
    let client = [("client_id", "fake-client"), ("client_secret", "fake-client-secret")];
    let fields =
        [("refresh_token", "fake-refresh"), ("auth_method", "idc"), ("region", "eu-west-1")];
    let run = mint_once(&kiro_recipe, &[&fields[..], &client[..]].concat(), &json!({ "idc": ok }));
    assert_eq!(run.recipe.as_deref(), Some("idc"));
    assert_eq!(run.requests[0].url, "https://oidc.eu-west-1.amazonaws.com/token");
    // Selected, but a param names an absent field: a configuration error, and nothing is sent.
    let run = mint_once(&kiro_recipe, &fields, &json!({ "idc": ok }));
    assert_eq!(run.recipe.as_deref(), Some("idc"));
    assert_eq!(
        serde_json::to_value(&run.outcome).unwrap(),
        json!({ "configuration": { "field": "client_id" } })
    );
    assert!(run.requests.is_empty());

    // Presence of a secret field selects too.
    let run = mint_once(
        &kiro_recipe,
        &[&[("refresh_token", "fake-refresh")][..], &client[..]].concat(),
        &json!({ "idc": ok }),
    );
    assert_eq!(run.recipe.as_deref(), Some("idc"));
    assert_eq!(
        run.requests[0].body.as_deref(),
        Some(
            "{\"clientId\":\"fake-client\",\"clientSecret\":\"fake-client-secret\",\
             \"grantType\":\"refresh_token\",\"refreshToken\":\"fake-refresh\"}"
        )
    );
}

#[test]
fn a_template_parameter_without_its_syntax_sends_nothing() {
    let kiro_recipe = credentials(&kiro());
    let run = mint_once(
        &kiro_recipe,
        &[("refresh_token", "fake-refresh"), ("region", "evil.example")],
        &json!({}),
    );
    assert_eq!(
        serde_json::to_value(&run.outcome).unwrap(),
        json!({ "configuration": { "field": "region" } })
    );
    assert!(run.requests.is_empty());
}

#[test]
fn an_exact_status_beats_its_class_and_the_defaults_apply_without_either() {
    let kiro_recipe = credentials(&kiro());
    let outcome_for = |status: u16| {
        let run = mint_once(
            &kiro_recipe,
            &[("refresh_token", "fake-refresh")],
            &json!({ "social": { "status": status, "body": null } }),
        );
        serde_json::to_value(&run.outcome).unwrap()
    };
    assert_eq!(outcome_for(401), json!({ "reauth_required": { "step": "social", "status": 401 } }));
    assert_eq!(outcome_for(403), json!({ "transient": { "step": "social", "status": 403 } }));
    assert_eq!(outcome_for(503), json!({ "transient": { "step": "social", "status": 503 } }));

    // Codex declares no on_status: 4xx defaults to reauth_required, 5xx to transient.
    let codex_recipe = credentials(&codex());
    let default_for = |status: u16| {
        let run =
            mint_once(&codex_recipe, &CODEX_FIELDS, &json!({ "refresh": { "status": status } }));
        serde_json::to_value(&run.outcome).unwrap()
    };
    assert_eq!(
        default_for(403),
        json!({ "reauth_required": { "step": "refresh", "status": 403 } })
    );
    assert_eq!(default_for(502), json!({ "transient": { "step": "refresh", "status": 502 } }));
}

fn copilot() -> Value {
    json!({
        "schema": "south.credential-recipe.v1",
        "fields": { "github_token": { "secret": true, "required": true } },
        "slots": { "provider_api_key": { "minted": "copilot" } },
        "recipes": {
            "copilot": {
                "steps": [
                    { "id": "exchange", "kind": "http_exchange", "method": "GET",
                      "endpoint": "https://api.github.com/copilot_internal/v2/token",
                      "auth": { "scheme": "token", "value": { "field": "github_token" } },
                      "headers": { "editor-version": { "const": "vscode/1.100.0" } },
                      "on_status": { "404": { "goto": "direct" } },
                      "extract": {
                          "token": { "pointer": "/token", "secret": true },
                          "expires_at": { "epoch_seconds": "/expires_at" } } },
                    { "id": "direct", "kind": "http_probe", "method": "GET",
                      "endpoint": "https://api.githubcopilot.com/models",
                      "auth": { "scheme": "Bearer", "value": { "field": "github_token" } } }
                ],
                "present": "exchange.token",
                "rotates_refresh_material": false
            }
        }
    })
}

#[test]
fn a_successful_step_does_not_fall_through_into_a_goto_branch() {
    let copilot_recipe = credentials(&copilot());
    let run = mint_once(
        &copilot_recipe,
        &[("github_token", "fake-gh")],
        &json!({ "exchange": { "status": 200, "body": { "token": "fake-copilot", "expires_at": NOW + 1500 } } }),
    );
    assert_eq!(
        serde_json::to_value(&run.requests).unwrap(),
        json!([{ "step": "exchange", "method": "GET",
                 "url": "https://api.github.com/copilot_internal/v2/token",
                 "headers": { "authorization": "token fake-gh", "editor-version": "vscode/1.100.0" } }])
    );
    assert_eq!(minted(&run).present, "fake-copilot");
    assert_eq!(minted(&run).expires_at, NOW + 1500);
}

#[test]
fn a_goto_runs_the_branch_and_its_probe_classifies_by_status() {
    let copilot_recipe = credentials(&copilot());
    let run = mint_once(
        &copilot_recipe,
        &[("github_token", "fake-gh")],
        &json!({ "exchange": { "status": 404 }, "direct": { "status": 401 } }),
    );
    assert_eq!(
        run.requests.iter().map(|request| request.step.as_str()).collect::<Vec<_>>(),
        ["exchange", "direct"]
    );
    assert_eq!(run.requests[1].headers["authorization"], "Bearer fake-gh");
    assert_eq!(
        serde_json::to_value(&run.outcome).unwrap(),
        json!({ "reauth_required": { "step": "direct", "status": 401 } })
    );
}

/// §3.9 says Copilot's direct-use flow is `on_status` 404 → `goto` + `http_probe`. In that flow
/// the host presents the GitHub token itself, but `present` names one step output, and the probe
/// extracts nothing — so the recipe has no way to say what it presents after the branch. Recorded
/// here as the vocabulary's gap rather than papered over.
#[test]
fn copilot_direct_use_cannot_name_its_presented_value() {
    let copilot_recipe = credentials(&copilot());
    let run = mint_once(
        &copilot_recipe,
        &[("github_token", "fake-gh")],
        &json!({ "exchange": { "status": 404 }, "direct": { "status": 200 } }),
    );
    assert_eq!(
        serde_json::to_value(&run.outcome).unwrap(),
        json!({ "transient": { "step": "exchange" } })
    );
}

fn vertex() -> Value {
    json!({
        "schema": "south.credential-recipe.v1",
        "fields": {
            "service_account": { "secret": true, "required": true, "media": "application/json" },
            "project_id": { "secret": false, "required": true, "syntax": "gcp_project_id" }
        },
        "slots": { "provider_api_key": { "minted": "vertex_sa" } },
        "recipes": {
            "vertex_sa": {
                "steps": [
                    { "id": "assertion", "kind": "jwt_sign", "alg": "RS256",
                      "key": { "field": "service_account", "pointer": "/private_key" },
                      "claims": {
                          "iss": { "field": "service_account", "pointer": "/client_email" },
                          "scope": { "const": "https://www.googleapis.com/auth/cloud-platform" },
                          "aud": { "endpoint_of": "token" },
                          "iat": { "now_plus": 0 }, "exp": { "now_plus": 3600 } } },
                    { "id": "token", "kind": "oauth2_token", "encoding": "form",
                      "endpoint": "https://oauth2.googleapis.com/token",
                      "params": {
                          "grant_type": { "const": "urn:ietf:params:oauth:grant-type:jwt-bearer" },
                          "assertion": { "output": "assertion.jwt" } },
                      "extract": {
                          "access_token": { "pointer": "/access_token", "secret": true },
                          "expires_at": { "relative_seconds": "/expires_in" } } }
                ],
                "present": "token.access_token",
                "rotates_refresh_material": false,
                "attributes": { "project_id": { "field": "project_id", "export": true } }
            }
        }
    })
}

const SERVICE_ACCOUNT: &str = r#"{"client_email":"sa@fake.iam","private_key":"fake-pem"}"#;

#[test]
fn a_signed_assertion_is_bound_to_the_step_it_is_sent_to() {
    let vertex_recipe = credentials(&vertex());
    let signer = HostBytes { signature: vec![1, 2, 3], seen: RefCell::default() };
    let fields: BTreeMap<String, String> = [
        ("service_account".to_owned(), SERVICE_ACCOUNT.to_owned()),
        ("project_id".to_owned(), "fake-project".to_owned()),
    ]
    .into();
    let mut responder = |_: &RenderedRequestV1| {
        Some(FakeResponseV1 {
            status: 200,
            body: json!({ "access_token": "fake-vertex", "expires_in": 3599 }),
        })
    };
    let run = run_recipe_v1(
        &vertex_recipe,
        "provider_api_key",
        &fields,
        &mut RecipeEffectsV1 { now: NOW, signer: &signer, responder: &mut responder },
    );
    // Registered claims first in RFC 7519 order, then the rest in byte order. (The host's own
    // Vertex struct orders them iss, scope, aud, iat, exp: equal JSON, different bytes.)
    let claims = format!(
        concat!(
            r#"{{"iss":"sa@fake.iam","aud":"https://oauth2.googleapis.com/token","exp":{},"#,
            r#""iat":{},"scope":"https://www.googleapis.com/auth/cloud-platform"}}"#
        ),
        NOW + 3600,
        NOW
    );
    let jwt = format!("{}.{}.AQID", b64(br#"{"typ":"JWT","alg":"RS256"}"#), b64(claims.as_bytes()));
    assert_eq!(signer.seen.borrow()[0].1, b"fake-pem", "the key is the pointer into the document");
    assert_eq!(
        run.requests[0].body.as_deref(),
        Some(
            format!(
                "assertion={jwt}&grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer"
            )
            .as_str()
        )
    );
    assert_eq!(run.requests[0].headers["content-type"], "application/x-www-form-urlencoded");
    assert_eq!(minted(&run).present, "fake-vertex");
    assert_eq!(minted(&run).expires_at, NOW + 3599);
    assert_eq!(minted(&run).attributes["project_id"], "fake-project");
}

#[test]
fn every_expiry_goes_through_the_clamp_narrowed_by_the_recipe() {
    let vertex_at = |expires_in: i64, max_ttl: Option<u32>| {
        let mut recipe = vertex();
        if let Some(max) = max_ttl {
            recipe["recipes"]["vertex_sa"]["max_ttl_seconds"] = json!(max);
        }
        let run = mint_once(
            &credentials(&recipe),
            &[("service_account", SERVICE_ACCOUNT), ("project_id", "fake-project")],
            &json!({ "token": { "status": 200, "body": { "access_token": "t", "expires_in": expires_in } } }),
        );
        minted(&run).expires_at - NOW
    };
    assert_eq!(vertex_at(172_800, None), 86_400, "the host's 24 h ceiling");
    assert_eq!(vertex_at(10, None), 60, "the host's 60 s floor");
    assert_eq!(vertex_at(-30, None), 60, "an expiry in the past is clamped too");
    assert_eq!(vertex_at(7200, Some(3600)), 3600, "the recipe narrows the ceiling");
}

#[test]
fn each_clock_convention_reads_its_own_unit_and_falls_back_only_to_the_default() {
    let with_clock = |clock: Value, default: Option<u32>, body: Value| {
        let mut recipe = copilot();
        recipe["recipes"]["copilot"]["steps"][0]["extract"]["expires_at"] = clock;
        if let Some(default) = default {
            recipe["recipes"]["copilot"]["default_seconds"] = json!(default);
        }
        let mut body = body;
        body["token"] = json!("fake-copilot");
        let run = mint_once(
            &credentials(&recipe),
            &[("github_token", "fake-gh")],
            &json!({ "exchange": { "status": 200, "body": body } }),
        );
        serde_json::to_value(&run.outcome).unwrap().pointer("/minted/expires_at").cloned()
    };
    let millis = NOW * 1000 + 1_800_999;
    assert_eq!(
        with_clock(json!({ "epoch_millis": "/at" }), None, json!({ "at": millis })),
        Some(json!(NOW + 1800))
    );
    assert_eq!(
        with_clock(
            json!({ "jwt_exp": "/token_jwt" }),
            None,
            json!({ "token_jwt": "eyJhbGciOiJub25lIn0.eyJleHAiOjE3NjcyMjkyMDB9.sig" })
        ),
        Some(json!(1_767_229_200))
    );
    // A value that is not a JWT falls back to default_seconds...
    assert_eq!(
        with_clock(json!({ "jwt_exp": "/token_jwt" }), Some(900), json!({ "token_jwt": "opaque" })),
        Some(json!(NOW + 900))
    );
    // ...and without one the run is transient.
    assert_eq!(with_clock(json!({ "relative_seconds": "/in" }), None, json!({})), None);
}

#[test]
fn a_static_slot_or_an_absent_field_mints_nothing() {
    let mut kling = shipped("task-kling-v2").credentials.unwrap();
    let run = mint_once(&kling, &[("access_key", "fake-access-key")], &json!({}));
    assert_eq!(
        serde_json::to_value(&run.outcome).unwrap(),
        json!({ "configuration": { "field": "secret_key" } })
    );
    kling.slots.clear();
    let run = mint_once(&kling, &[("access_key", "a"), ("secret_key", "s")], &json!({}));
    assert!(matches!(run.outcome, RecipeOutcomeV1::Configuration { field: None, .. }));
}

// ---------------------------------------------------------------------------------------------
// The check and its coverage rule.
// ---------------------------------------------------------------------------------------------

fn case(family: CredentialFamilyV1, name: &str, input: Value, expected: Value) -> CredentialCaseV1 {
    CredentialCaseV1 {
        name: format!("credential.{}.{name}", family.token()),
        family,
        input,
        expected,
    }
}

fn codex_input(responses: &Value) -> Value {
    json!({ "slot": "provider_api_key", "now": NOW,
            "fields": { "refresh_token": "fake-refresh-1", "account_id": "acct-1" },
            "responses": responses })
}

/// A complete Codex pack: rotation, an `on_status` transition, and a clock sample (which is also
/// the no-wipe sample).
fn codex_pack() -> Vec<CredentialCaseV1> {
    let mut rejected = codex_rotation_expected();
    rejected["outcome"] = json!({ "reauth_required": { "step": "refresh", "status": 401 } });
    let mut kept = codex_rotation_expected();
    kept["outcome"]["minted"]["outputs"].as_object_mut().unwrap().remove("refresh.refresh_token");
    kept["outcome"]["minted"]["write_back"] = json!({});
    vec![
        case(
            CredentialFamilyV1::Rotation,
            "refresh",
            codex_input(&codex_rotation_response()),
            codex_rotation_expected(),
        ),
        case(
            CredentialFamilyV1::OnStatus,
            "rejected",
            codex_input(
                &json!({ "refresh": { "status": 401, "body": { "error": "invalid_grant" } } }),
            ),
            rejected,
        ),
        case(
            CredentialFamilyV1::Clock,
            "fixed-window-keeps-refresh-token",
            codex_input(&json!({ "refresh": { "status": 200, "body": {
                "access_token": "fake-access-2", "id_token": ID_TOKEN_ACCT_1, "expires_in": 7200 } } })),
            kept,
        ),
    ]
}

fn checks(recipe: &Value, cases: Vec<CredentialCaseV1>) -> Vec<OutcomeV1> {
    credential_recipe_checks_v1(&credentials(recipe), &CredentialFixturePackV1::from_cases(cases))
}

fn failures(outcomes: &[OutcomeV1]) -> Vec<(CheckV1, String)> {
    outcomes
        .iter()
        .filter(|outcome| outcome.is_failure())
        .map(|outcome| (outcome.check, outcome.case.clone()))
        .collect()
}

#[test]
fn a_complete_pack_passes_and_every_case_is_named() {
    let outcomes = checks(&codex(), codex_pack());
    assert_eq!(failures(&outcomes), [], "{outcomes:?}");
    let matched: Vec<&str> = outcomes
        .iter()
        .filter(|outcome| outcome.check == CheckV1::CredentialRecipeMatch)
        .map(|outcome| outcome.case.as_str())
        .collect();
    assert_eq!(
        matched,
        [
            "credential.rotation.refresh",
            "credential.on-status.rejected",
            "credential.clock.fixed-window-keeps-refresh-token"
        ]
    );
    assert!(
        outcomes.iter().any(|outcome| outcome.check == CheckV1::Coverage && !outcome.is_failure())
    );
}

#[test]
fn a_fixture_expecting_a_different_request_is_red() {
    let mut cases = codex_pack();
    cases[0].expected["requests"][0]["headers"]["content-type"] =
        json!("application/x-www-form-urlencoded");
    let outcomes = checks(&codex(), cases);
    assert_eq!(
        failures(&outcomes),
        [(CheckV1::CredentialRecipeMatch, "credential.rotation.refresh".to_owned())]
    );
}

#[test]
fn a_recipe_that_loses_or_misplaces_refresh_material_is_red() {
    // Declared as not rotating: the new refresh token is never written back.
    let mut forgetful = codex();
    let recipe = &mut forgetful["recipes"]["codex"];
    recipe["rotates_refresh_material"] = json!(false);
    recipe.as_object_mut().unwrap().remove("write_back");
    let outcomes = checks(&forgetful, codex_pack());
    assert!(
        failures(&outcomes)
            .contains(&(CheckV1::CredentialRecipeMatch, "credential.rotation.refresh".to_owned())),
        "{outcomes:?}"
    );

    // Writing the access token into the refresh material.
    let mut misplaced = codex();
    misplaced["recipes"]["codex"]["write_back"] =
        json!({ "refresh_token": "refresh.access_token" });
    let outcomes = checks(&misplaced, codex_pack());
    let red = failures(&outcomes);
    assert!(
        red.contains(&(CheckV1::CredentialRecipeMatch, "credential.rotation.refresh".to_owned()))
    );
    assert!(
        red.contains(&(
            CheckV1::CredentialRecipeMatch,
            "credential.clock.fixed-window-keeps-refresh-token".to_owned()
        )),
        "{outcomes:?}"
    );
}

#[test]
fn a_fixture_expecting_a_wipe_cannot_pass() {
    let mut cases = codex_pack();
    cases[2].expected["outcome"]["minted"]["write_back"] = json!({ "refresh_token": "" });
    let outcomes = checks(&codex(), cases);
    assert!(failures(&outcomes).contains(&(
        CheckV1::CredentialRecipeMatch,
        "credential.clock.fixed-window-keeps-refresh-token".to_owned()
    )));
}

#[test]
fn a_missing_or_mislabelled_sample_is_a_coverage_failure_by_name() {
    let mut cases = codex_pack();
    cases.remove(1);
    let outcomes = checks(&codex(), cases);
    assert_eq!(failures(&outcomes), [(CheckV1::Coverage, "credential.on-status".to_owned())]);

    // A case labelled rotation that does not mint through a rotating recipe covers nothing.
    let mut cases = codex_pack();
    cases.remove(0);
    let mut relabelled = cases.remove(0);
    relabelled.family = CredentialFamilyV1::Rotation;
    relabelled.name = "credential.rotation.rejected".to_owned();
    cases.push(relabelled);
    let red = failures(&checks(&codex(), cases));
    assert!(red.contains(&(CheckV1::Coverage, "credential.rotation".to_owned())), "{red:?}");
    assert!(red.contains(&(CheckV1::Coverage, "credential.on-status".to_owned())), "{red:?}");

    // An empty pack owes every sample its recipes call for.
    let red = failures(&checks(&codex(), Vec::new()));
    assert_eq!(
        red,
        [
            (CheckV1::Coverage, "credential.rotation".to_owned()),
            (CheckV1::Coverage, "credential.on-status".to_owned()),
            (CheckV1::Coverage, "credential.clock".to_owned()),
        ]
    );
}

#[test]
fn a_fixture_written_for_another_recipe_is_red() {
    let mut cases = codex_pack();
    cases[0].input["responses"]["exchange"] = json!({ "status": 200 });
    let red = failures(&checks(&codex(), cases));
    assert_eq!(red, [(CheckV1::CredentialRecipeMatch, "credential.rotation.refresh".to_owned())]);

    let mut cases = codex_pack();
    // An input that does not parse is red, and covers nothing.
    cases[0].input["nonce"] = json!(1);
    let red = failures(&checks(&codex(), cases));
    assert_eq!(
        red,
        [
            (CheckV1::Coverage, "credential.rotation".to_owned()),
            (CheckV1::CredentialRecipeMatch, "credential.rotation.refresh".to_owned())
        ]
    );
}

#[test]
fn the_provider_suite_runs_the_credential_checks_for_a_manifest_with_recipes() {
    let fixtures = root().join("fixtures");
    let pack = FixturePackV1::load(&fixtures)
        .unwrap()
        .with_credentials(CredentialFixturePackV1::from_cases(codex_pack()));
    let manifest = manifest_with(&codex());
    let report = run_provider_component_suite_v1_for_manifest(
        &OpenAiCompatibleReferenceV1,
        &pack,
        &manifest,
    );
    assert!(report.is_passing(), "{report}");
    assert_eq!(
        report.outcomes().iter().filter(|o| o.check == CheckV1::CredentialRecipeMatch).count(),
        3
    );

    // The same pack without its credential cases fails coverage by name.
    let bare = FixturePackV1::load(&fixtures).unwrap();
    let report = run_provider_component_suite_v1_for_manifest(
        &OpenAiCompatibleReferenceV1,
        &bare,
        &manifest,
    );
    assert!(
        report.failures().any(|o| o.check == CheckV1::Coverage && o.case == "credential.clock")
    );

    // A manifest without credentials owes none.
    let report = run_provider_component_suite_v1_for_manifest(
        &OpenAiCompatibleReferenceV1,
        &bare,
        &shipped("provider-openai-compatible"),
    );
    assert!(report.is_passing(), "{report}");
}
