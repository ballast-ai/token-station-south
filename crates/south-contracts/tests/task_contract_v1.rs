//! Public contract tests for the task adapter vocabulary (ruling D1–D3 of
//! `docs/design/2026-08-27-task-adapter-vocabulary.md`).

use south_contracts::{
    HostMintedValuesV1, MAX_CALLBACK_URL_BYTES, MAX_TASK_ID_BYTES, TaskContractErrorV1,
    TaskFailureKindV1, TaskObservationV1,
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
    assert_eq!(TaskObservationV1::Running.state_word(), "running");
    assert_eq!(TaskObservationV1::Succeeded.state_word(), "succeeded");
    assert_eq!(TaskObservationV1::Failed(TaskFailureKindV1::Failed).state_word(), "failed");
    assert_eq!(TaskObservationV1::Unknown.state_word(), "unknown");

    let words: Vec<&str> = TaskFailureKindV1::ALL.iter().map(|kind| kind.word()).collect();
    assert_eq!(words, ["failed", "cancelled", "provider-expired"]);
}

/// Rule 3 of D3: `unknown` is a reconciliation input, never a terminal — a
/// host that settles on it releases the fee for a task that may still be
/// running.
#[test]
fn unknown_is_never_terminal() {
    assert!(!TaskObservationV1::Running.is_terminal());
    assert!(!TaskObservationV1::Unknown.is_terminal());
    assert!(TaskObservationV1::Succeeded.is_terminal());
    for kind in TaskFailureKindV1::ALL {
        assert!(TaskObservationV1::Failed(kind).is_terminal());
    }
}
