//! Public contract tests for the task adapter vocabulary (ruling D1–D3 of
//! `docs/design/2026-08-27-task-adapter-vocabulary.md`).

use south_contracts::{
    HostMintedValuesV1, MAX_CALLBACK_URL_BYTES, MAX_TASK_ID_BYTES, TaskArtifactRefV1,
    TaskContractErrorV1, TaskFailureKindV1, TaskMeterV1, TaskObservationV1,
};

#[test]
fn host_minted_values_stay_byte_exact() {
    let minted = HostMintedValuesV1::new(
        "task-01J9ZK3V7Q",
        Some("https://gateway.example/callback/01J9ZK3V7Q?nonce=n-4242"),
    )
    .expect("a well-formed declaration validates");
    assert_eq!(minted.task_id(), "task-01J9ZK3V7Q");
    assert_eq!(
        minted.callback_url(),
        Some("https://gateway.example/callback/01J9ZK3V7Q?nonce=n-4242")
    );

    let no_callback = HostMintedValuesV1::new("task-01J9ZK3V7Q", None)
        .expect("a dialect with no callback concept omits the option");
    assert_eq!(no_callback.callback_url(), None);
}

#[test]
fn each_refused_shape_is_named() {
    assert_eq!(HostMintedValuesV1::new("", None), Err(TaskContractErrorV1::EmptyTaskId));
    assert_eq!(
        HostMintedValuesV1::new(&"t".repeat(MAX_TASK_ID_BYTES + 1), None),
        Err(TaskContractErrorV1::TaskIdTooLarge)
    );
    for bad_id in ["task id", "task\nid", "task\u{7f}id", "täsk"] {
        assert_eq!(
            HostMintedValuesV1::new(bad_id, None),
            Err(TaskContractErrorV1::TaskIdNotPrintableAscii),
            "task id {bad_id:?} must be refused"
        );
    }
    assert_eq!(
        HostMintedValuesV1::new("task", Some("")),
        Err(TaskContractErrorV1::EmptyCallbackUrl)
    );
    assert_eq!(
        HostMintedValuesV1::new("task", Some(&"u".repeat(MAX_CALLBACK_URL_BYTES + 1))),
        Err(TaskContractErrorV1::CallbackUrlTooLarge)
    );
    for bad_url in ["https://a b", "https://a\nb", "https://a\u{0}b"] {
        assert_eq!(
            HostMintedValuesV1::new("task", Some(bad_url)),
            Err(TaskContractErrorV1::CallbackUrlNotUrlSafe),
            "callback URL {bad_url:?} must be refused"
        );
    }
}

/// Rule 3 of the D2 contract: the nonce plaintext exists only inside the
/// URL, so nothing this type prints may carry it.
#[test]
fn debug_prints_byte_counts_and_never_a_value() {
    let minted = HostMintedValuesV1::new("task-1", Some("https://g.example/cb?nonce=secret-n"))
        .expect("valid declaration");
    let printed = format!("{minted:?}");
    assert!(!printed.contains("task-1"), "task id leaked: {printed}");
    assert!(!printed.contains("secret-n"), "nonce leaked: {printed}");
    assert!(printed.contains("byte_count"), "byte counts are the only detail: {printed}");
}

/// D3's shape: expiry is a failure kind, not an observation variant, and the
/// vocabulary words are frozen.
#[test]
fn the_observation_vocabulary_is_frozen() {
    // The four words are the frozen part. Contract version two gave three of
    // the arms a payload; the words they answer with did not move.
    assert_eq!(running("processing").state_word(), "running");
    assert_eq!(succeeded_bare().state_word(), "succeeded");
    assert_eq!(failed(TaskFailureKindV1::Failed).state_word(), "failed");
    assert_eq!(unknown("query http 500").state_word(), "unknown");

    let words: Vec<&str> = TaskFailureKindV1::ALL.iter().map(|kind| kind.word()).collect();
    assert_eq!(words, ["failed", "cancelled", "provider-expired"]);
}

/// Rule 3 of D3: `unknown` is a reconciliation input, never a terminal — a
/// host that settles on it releases the fee for a task that may still be
/// running.
#[test]
fn unknown_is_never_terminal() {
    assert!(!running("processing").is_terminal());
    assert!(!unknown("query http 429").is_terminal());
    assert!(succeeded_bare().is_terminal());
    for kind in TaskFailureKindV1::ALL {
        assert!(failed(kind).is_terminal());
    }
}

// ── Constructors for the arms, so a payload change stays one edit ───────────

fn running(word: &str) -> TaskObservationV1 {
    TaskObservationV1::Running { status_word: word.to_owned() }
}

const fn succeeded_bare() -> TaskObservationV1 {
    TaskObservationV1::Succeeded { artifact: TaskArtifactRefV1::None, meter: None }
}

const fn failed(kind: TaskFailureKindV1) -> TaskObservationV1 {
    TaskObservationV1::Failed { kind, code: None, message: None }
}

fn unknown(reason: &str) -> TaskObservationV1 {
    TaskObservationV1::Unknown { reason: reason.to_owned() }
}

// ── Contract version two: the terminal arms carry what the upstream reported
//    (2026-09-19 vocabulary fit survey) ──────────────────────────────────────

/// A component must be able to say *what* succeeded, not merely that something
/// did. Without this the host would have to parse the dialect's terminal body
/// itself — the exact knowledge the component exists to hold.
#[test]
fn a_succeeded_observation_carries_the_artifact_it_produced() {
    let urls = TaskArtifactRefV1::urls(vec!["https://cdn.example/v/1.mp4".to_owned()])
        .expect("one url is a valid reference");
    let observation = TaskObservationV1::Succeeded { artifact: urls.clone(), meter: None };
    assert!(observation.is_terminal());
    assert_eq!(observation.state_word(), "succeeded");
    let TaskObservationV1::Succeeded { artifact, .. } = observation else {
        panic!("succeeded carries its artifact");
    };
    assert_eq!(artifact, urls);
}

/// One family answers with an id instead of a URL — the reason
/// `build-artifact-request` exists at all.
#[test]
fn an_artifact_may_be_an_id_the_host_must_fetch() {
    let by_id = TaskArtifactRefV1::file_id("file-01J9ZK").expect("a well-formed id");
    assert!(matches!(by_id, TaskArtifactRefV1::FileId(_)));
}

/// The meter is what the upstream *reported*, in the unit it reported it.
///
/// A closed set on purpose: a free map is how a metering vocabulary becomes a
/// pricing vocabulary one key at a time. Pricing stays host-side.
#[test]
fn a_reported_meter_is_a_closed_set_of_named_quantities() {
    for meter in
        [TaskMeterV1::Seconds(8.0), TaskMeterV1::Tokens(1_920), TaskMeterV1::Milliunits(4_500)]
    {
        let observation =
            TaskObservationV1::Succeeded { artifact: TaskArtifactRefV1::None, meter: Some(meter) };
        let TaskObservationV1::Succeeded { meter: Some(reported), .. } = observation else {
            panic!("the meter round-trips");
        };
        assert_eq!(reported, meter);
    }
}

/// An upstream that reports no meter is normal, not an error: most families
/// bill from the request, not the result.
#[test]
fn a_missing_meter_is_absence_not_zero() {
    let observation =
        TaskObservationV1::Succeeded { artifact: TaskArtifactRefV1::None, meter: None };
    let TaskObservationV1::Succeeded { meter, .. } = observation else {
        panic!("succeeded");
    };
    assert_eq!(meter, None);
}

/// The host surfaces the upstream's own words; `map-terminal-failure` maps
/// them onto the closed `ErrorCode` catalog. Neither can happen if the
/// observation drops them.
#[test]
fn a_failed_observation_carries_the_upstreams_own_words() {
    let observation = TaskObservationV1::Failed {
        kind: TaskFailureKindV1::ProviderExpired,
        code: Some("task_expired".to_owned()),
        message: Some("task result expired after 24h".to_owned()),
    };
    assert!(observation.is_terminal());
    assert_eq!(observation.state_word(), "failed");
}

/// Rules 1–3 are unchanged by this version: the non-terminal arms still carry
/// only what diagnosis needs, and neither is ever terminal.
#[test]
fn the_non_terminal_arms_keep_their_reason_without_becoming_terminal() {
    let running = TaskObservationV1::Running { status_word: "processing".to_owned() };
    let unknown = TaskObservationV1::Unknown { reason: "query http 429".to_owned() };
    assert!(!running.is_terminal());
    assert!(!unknown.is_terminal());
    assert_eq!(running.state_word(), "running");
    assert_eq!(unknown.state_word(), "unknown");
}
