//! Credential recipe v1 at gate ① (B4, `docs/design/2026-09-30-host-zero-vendor-boundary.md` §3.3,
//! §3.4, §3.7, §3.9): the vocabulary expresses every family the host mints today, and every
//! structural and trust rule refuses what it guards.

use std::path::Path;

use serde_json::{Value, json};
use south_provider_api::{ComponentManifestV1, ManifestErrorV1};

fn shipped(package: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../components/{package}/manifest.json"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// A shipped manifest carrying `credentials`, parsed and validated.
fn with(package: &str, credentials: &Value) -> Result<(), ManifestErrorV1> {
    let mut manifest = shipped(package);
    manifest["credentials"] = credentials.clone();
    let manifest: ComponentManifestV1 = serde_json::from_value(manifest)
        .map_err(|error| ManifestErrorV1::InvalidCredentials(error.to_string()))?;
    manifest.validate()
}

fn vertex() -> Value {
    json!({
        "schema": "south.credential-recipe.v1",
        "fields": {
            "service_account": { "secret": true, "required": true, "media": "application/json" },
            "project_id": { "secret": false, "required": true, "syntax": "gcp_project_id" }
        },
        "import": {
            "service_account": { "file": "service-account-json", "pointers": [""] },
            "project_id": { "file": "service-account-json", "pointers": ["/project_id"] }
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

fn kling() -> Value {
    json!({
        "schema": "south.credential-recipe.v1",
        "fields": {
            "access_key": { "secret": false, "required": true, "syntax": { "printable_ascii": 128 } },
            "secret_key": { "secret": true, "required": true }
        },
        "slots": { "provider_api_key": { "minted": "kling_jwt" } },
        "recipes": {
            "kling_jwt": {
                "steps": [
                    { "id": "jwt", "kind": "jwt_sign", "alg": "HS256",
                      "key": { "field": "secret_key" },
                      "claims": {
                          "iss": { "field": "access_key" },
                          "exp": { "now_plus": 1800 },
                          "nbf": { "now_plus": -5 } } }
                ],
                "present": "jwt.jwt",
                "rotates_refresh_material": false,
                "default_seconds": 1800
            }
        }
    })
}

fn codex() -> Value {
    json!({
        "schema": "south.credential-recipe.v1",
        "fields": {
            "access_token": { "secret": true },
            "refresh_token": { "secret": true },
            "account_id": { "secret": false, "syntax": { "printable_ascii": 128 } }
        },
        "require_one_of": [["access_token", "refresh_token"]],
        "import": {
            "access_token": { "file": "codex-auth-json", "pointers": ["/tokens/access_token"] },
            "refresh_token": { "file": "codex-auth-json", "pointers": ["/tokens/refresh_token"] },
            "account_id": { "file": "codex-auth-json", "pointers": ["/tokens/account_id"] }
        },
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
                              "jwt_claim": { "token": "/id_token", "pointer": "/https:~1~1api.openai.com~1auth/chatgpt_account_id" },
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
        "seed": { "file": "kiro-auth-json", "slot": "provider_api_key", "present": "/accessToken",
                  "expires_at": { "rfc3339_or_epoch_seconds": "/expiresAt" } },
        "slots": { "provider_api_key": { "minted": "kiro" } },
        "recipes": {
            "kiro": { "select": [
                { "when": { "field_in": { "field": "auth_method", "values": ["idc"] } }, "recipe": "idc" },
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

#[test]
fn the_vocabulary_expresses_every_family_the_host_mints_today() {
    for (family, package, credentials) in [
        ("vertex", "provider-gemini", vertex()),
        ("codex", "provider-openai-compatible", codex()),
        ("copilot", "provider-openai-compatible", copilot()),
        ("kiro", "provider-openai-compatible", kiro()),
        ("kling", "task-kling-v2", kling()),
    ] {
        assert_eq!(with(package, &credentials), Ok(()), "{family}");
    }
}

#[test]
fn every_reachable_endpoint_is_listed_for_operator_confirmation() {
    let manifest: ComponentManifestV1 = {
        let mut manifest = shipped("provider-openai-compatible");
        manifest["credentials"] = kiro();
        serde_json::from_value(manifest).unwrap()
    };
    assert_eq!(
        manifest.credentials.unwrap().endpoints(),
        [
            "https://oidc.{region}.amazonaws.com/token",
            "https://prod.{region}.auth.desktop.kiro.dev/refreshToken",
        ]
    );
}

/// Sets `pointer` in `value`, creating the last key if it is missing.
fn insert(value: &mut Value, pointer: &str, new: Value) {
    let (parent, key) = pointer.rsplit_once('/').expect("a member pointer");
    let key = key.replace("~1", "/").replace("~0", "~");
    value
        .pointer_mut(parent)
        .and_then(Value::as_object_mut)
        .unwrap_or_else(|| panic!("no object at `{parent}`"))
        .insert(key, new);
}

/// Applies `edit` to a valid recipe and requires gate ① to refuse it.
fn refuses(name: &str, package: &str, base: Value, edit: impl Fn(&mut Value)) {
    let mut credentials = base;
    edit(&mut credentials);
    let refused = with(package, &credentials);
    assert!(matches!(refused, Err(ManifestErrorV1::InvalidCredentials(_))), "{name}: {refused:?}");
}

#[test]
fn structural_rules_refuse_what_they_guard() {
    let codex_recipe = "/recipes/codex";
    let set = insert;
    let remove = |value: &mut Value, parent: &str, key: &str| {
        value.pointer_mut(parent).unwrap().as_object_mut().unwrap().remove(key);
    };
    let package = "provider-openai-compatible";

    refuses("wrong schema", package, codex(), |c| {
        set(c, "/schema", json!("south.credential-recipe.v2"));
    });
    refuses("rotation has no default", package, codex(), |c| {
        remove(c, codex_recipe, "rotates_refresh_material");
    });
    refuses("rotates without write_back", package, codex(), |c| {
        remove(c, codex_recipe, "write_back");
    });
    refuses("write_back without rotating", package, codex(), |c| {
        set(c, &format!("{codex_recipe}/rotates_refresh_material"), json!(false));
    });
    refuses("write_back into a non-secret field", package, codex(), |c| {
        set(
            c,
            &format!("{codex_recipe}/write_back"),
            json!({ "account_id": "refresh.refresh_token" }),
        );
    });
    refuses("present names nothing", package, codex(), |c| {
        set(c, &format!("{codex_recipe}/present"), json!("refresh.nothing"));
    });
    refuses("fixed_window without its window", package, codex(), |c| {
        remove(c, codex_recipe, "fixed_validity_seconds");
    });
    refuses("two sources for one extraction", package, codex(), |c| {
        set(
            c,
            &format!("{codex_recipe}/steps/0/extract/expires_at"),
            json!({ "fixed_window": true, "epoch_seconds": "/exp" }),
        );
    });
    refuses("must_equal an undeclared field", package, codex(), |c| {
        set(
            c,
            &format!("{codex_recipe}/steps/0/extract/account/must_equal_field"),
            json!("nobody"),
        );
    });
    refuses("a TTL bound outside the host clamp", package, codex(), |c| {
        set(c, &format!("{codex_recipe}/max_ttl_seconds"), json!(172_800));
    });
    refuses("min above max", package, codex(), |c| {
        set(c, &format!("{codex_recipe}/min_ttl_seconds"), json!(600));
        set(c, &format!("{codex_recipe}/max_ttl_seconds"), json!(300));
    });
    refuses("five steps", package, codex(), |c| {
        let step = c.pointer(&format!("{codex_recipe}/steps/0")).unwrap().clone();
        let steps: Vec<Value> = (0..5)
            .map(|index| {
                let mut step = step.clone();
                step["id"] = json!(format!("s{index}"));
                step
            })
            .collect();
        set(c, &format!("{codex_recipe}/steps"), json!(steps));
    });
    refuses("oauth2_token as GET", package, codex(), |c| {
        set(c, &format!("{codex_recipe}/steps/0/method"), json!("GET"));
    });
    refuses("plain http", package, codex(), |c| {
        set(
            c,
            &format!("{codex_recipe}/steps/0/endpoint"),
            json!("http://auth.openai.com/oauth/token"),
        );
    });
    refuses("a reserved header", package, codex(), |c| {
        set(
            c,
            &format!("{codex_recipe}/steps/0/headers"),
            json!({ "authorization": { "const": "x" } }),
        );
    });
    refuses("requires an undeclared field", package, codex(), |c| {
        set(c, &format!("{codex_recipe}/steps/0/requires"), json!(["nobody"]));
    });
    refuses("a slot not under permissions.secrets", package, codex(), |c| {
        set(c, "/slots", json!({ "other_slot": { "minted": "codex" } }));
    });
    refuses("a minted slot naming no recipe", package, codex(), |c| {
        set(c, "/slots", json!({ "provider_api_key": { "minted": "nothing" } }));
    });
    refuses("a default on a secret field", package, codex(), |c| {
        set(c, "/fields/refresh_token/default", json!("x"));
    });
    refuses("an unknown step kind", package, codex(), |c| {
        set(c, &format!("{codex_recipe}/steps/0/kind"), json!("ssh_exchange"));
    });
}

#[test]
fn status_rules_refuse_what_they_guard() {
    let set = insert;
    let package = "provider-openai-compatible";
    let copilot_steps = "/recipes/copilot/steps";
    refuses("goto backwards", package, copilot(), |c| {
        set(c, &format!("{copilot_steps}/1/on_status"), json!({ "404": { "goto": "exchange" } }));
    });
    refuses("an on_status key that is not a status", package, copilot(), |c| {
        set(c, &format!("{copilot_steps}/0/on_status"), json!({ "40x": "transient" }));
    });
}

#[test]
fn trust_rules_refuse_what_they_guard() {
    let vertex_steps = "/recipes/vertex_sa/steps";
    let set = insert;
    let package = "provider-gemini";

    // Rule 2: an assertion sent to a step is bound to that step's endpoint.
    refuses("an assertion without aud", package, vertex(), |c| {
        c.pointer_mut(&format!("{vertex_steps}/0/claims"))
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("aud");
    });
    refuses("an assertion with a constant aud", package, vertex(), |c| {
        set(
            c,
            &format!("{vertex_steps}/0/claims/aud"),
            json!({ "const": "https://oauth2.googleapis.com/token" }),
        );
    });
    refuses("endpoint_of naming no HTTP step", package, vertex(), |c| {
        set(c, &format!("{vertex_steps}/0/claims/aud"), json!({ "endpoint_of": "assertion" }));
    });
    // Rule 3: no constant subject.
    refuses("a constant sub", package, vertex(), |c| {
        set(c, &format!("{vertex_steps}/0/claims/sub"), json!({ "const": "admin@example.com" }));
    });
    // Rule 4: export only from non-secret fields.
    refuses("an attribute from a secret field", package, vertex(), |c| {
        set(
            c,
            "/recipes/vertex_sa/attributes",
            json!({ "key": { "field": "service_account", "export": true } }),
        );
    });
    refuses("a signing key from a non-secret field", package, vertex(), |c| {
        set(c, &format!("{vertex_steps}/0/key"), json!({ "field": "project_id" }));
    });
    refuses("endpoint_of outside aud", package, vertex(), |c| {
        set(c, &format!("{vertex_steps}/1/params/audience"), json!({ "endpoint_of": "token" }));
    });

    // §3.3: an endpoint's host is never chosen by credential contents.
    let kiro_social = "/recipes/social/steps/0";
    let package = "provider-openai-compatible";
    refuses("a host parameter from a secret field", package, kiro(), |c| {
        set(
            c,
            &format!("{kiro_social}/endpoint_params/region"),
            json!({ "field": "refresh_token" }),
        );
    });
    refuses("a host parameter whose syntax admits dots", package, kiro(), |c| {
        set(c, &format!("{kiro_social}/endpoint_params/region"), json!({ "field": "client_id" }));
    });
    refuses("a parameter the template does not have", package, kiro(), |c| {
        set(c, &format!("{kiro_social}/endpoint_params/zone"), json!({ "field": "region" }));
    });
    refuses("a template steered by a parameter", package, kiro(), |c| {
        set(c, &format!("{kiro_social}/endpoint"), json!("https://{region}/refreshToken"));
    });
    // A selector may branch on a secret's presence, never on its value.
    refuses("field_in on a secret field", package, kiro(), |c| {
        set(
            c,
            "/recipes/kiro/select/0/when",
            json!({ "field_in": { "field": "client_secret", "values": ["x"] } }),
        );
    });
    refuses("a selector with steps", package, kiro(), |c| {
        let steps = c.pointer("/recipes/social/steps").unwrap().clone();
        set(c, "/recipes/kiro/steps", steps);
    });
    refuses("a selector whose last rule tests", package, kiro(), |c| {
        set(c, "/recipes/kiro/select/2/when", json!({ "field_present": "client_id" }));
    });
    refuses("a selector naming a selector", package, kiro(), |c| {
        set(c, "/recipes/kiro/select/2/recipe", json!("kiro"));
    });
}
