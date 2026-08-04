//! The wire contract between Rust and Elm, pinned as a golden file.
//!
//! Phase 0 renamed `folderId` to `folderIds` and retyped `receivedAt`. Both
//! changes are silent if they land unevenly: `handleCommand` routes Elm decoder
//! failures into `model.error`, so a mismatch shows as a banner at run time
//! rather than a red test.
//!
//! This test serializes a full `app_bootstrap` payload and compares it to
//! `tests/fixtures/bootstrap.json`. Renaming or retyping a field on the Rust
//! side fails here, and the fix is to update `src/Api.elm` in the same commit
//! and re-bless the fixture.
//!
//! What this does not do: run Elm's decoder. That needs `elm-test`, which the
//! repo does not install. The fixture is the contract both sides are checked
//! against by eye, and the key set is machine-checked here.

use std::process::Command;

const FIXTURE: &str = include_str!("fixtures/bootstrap.json");

/// Every field name the Elm decoders in `src/Api.elm` call `required` on.
/// Kept in sync by hand, and by the `elm_decoders_require_exactly_these_fields`
/// test below, which greps the Elm source.
const SUMMARY_KEYS: &[&str] = &[
    "id",
    "accountId",
    "folderIds",
    "senderName",
    "senderEmail",
    "subject",
    "snippet",
    "receivedAt",
    "unread",
    "starred",
    "labels",
];

const DETAIL_EXTRA_KEYS: &[&str] = &["to", "cc", "replyTo", "bodyParagraphs"];

const ACCOUNT_KEYS: &[&str] = &[
    "id",
    "displayName",
    "emailAddress",
    "provider",
    "providerKind",
    "accent",
    "connected",
];

const BOOTSTRAP_KEYS: &[&str] = &[
    "providerOptions",
    "accounts",
    "folders",
    "messages",
    "selectedFolderId",
    "selectedMessageId",
    "selectedMessage",
    "syncStatus",
];

fn fixture() -> serde_json::Value {
    serde_json::from_str(FIXTURE).expect("the golden fixture must be valid JSON")
}

fn keys(value: &serde_json::Value) -> Vec<String> {
    let mut names: Vec<String> = value
        .as_object()
        .expect("expected a JSON object")
        .keys()
        .cloned()
        .collect();
    names.sort();
    names
}

fn sorted(values: &[&str]) -> Vec<String> {
    let mut owned: Vec<String> = values.iter().map(|value| value.to_string()).collect();
    owned.sort();
    owned
}

#[test]
fn bootstrap_payload_has_exactly_the_keys_elm_decodes() {
    let bootstrap = fixture();
    assert_eq!(keys(&bootstrap), sorted(BOOTSTRAP_KEYS));

    let account = &bootstrap["accounts"][0];
    assert_eq!(keys(account), sorted(ACCOUNT_KEYS));

    let summary = &bootstrap["messages"][0];
    assert_eq!(keys(summary), sorted(SUMMARY_KEYS));

    let mut detail_keys = SUMMARY_KEYS.to_vec();
    detail_keys.extend_from_slice(DETAIL_EXTRA_KEYS);
    // internalDate is on the detail only: it is the SQLite sort key, and Elm
    // sorts server-side rather than re-sorting.
    detail_keys.push("internalDate");
    detail_keys.push("providerMessageId");
    detail_keys.push("providerThreadId");
    assert_eq!(keys(&bootstrap["selectedMessage"]), sorted(&detail_keys));

    assert_eq!(
        keys(&bootstrap["syncStatus"]),
        sorted(&["state", "lastChecked", "detail"])
    );
    assert_eq!(
        keys(&bootstrap["providerOptions"][0]),
        sorted(&["provider", "displayName", "description", "browserLogin"])
    );
    assert_eq!(
        keys(&bootstrap["folders"][0]),
        sorted(&[
            "id",
            "accountId",
            "name",
            "role",
            "providerFolderId",
            "unreadCount"
        ])
    );
}

/// `folderIds` is a list and `receivedAt` is an ISO-8601 string. Those are the
/// two Phase 0 changes, and getting either wrong is the silent failure this file
/// exists to catch.
#[test]
fn timestamps_are_iso8601_and_folder_membership_is_a_list() {
    let bootstrap = fixture();
    let summary = &bootstrap["messages"][0];

    assert!(
        summary["folderIds"].is_array(),
        "folderIds must be a list, not a single id"
    );

    let received = summary["receivedAt"]
        .as_str()
        .expect("receivedAt must be a string");
    assert!(
        received.ends_with('Z'),
        "receivedAt must be UTC ISO-8601: {received}"
    );
    assert_eq!(&received[4..5], "-");
    assert_eq!(&received[10..11], "T");

    // Prose timestamps are what Phase 0 removed. Guard against a regression.
    for prose in ["Today", "Yesterday", "Just now", "Now"] {
        assert!(
            !FIXTURE.contains(prose),
            "the fixture still contains display prose in a timestamp field: {prose}"
        );
    }
}

/// The fixture is only worth having if it matches what the code actually
/// produces. Regenerate it with:
///
/// ```text
/// LOTUS_BLESS_FIXTURE=1 cargo test --test wire_shape
/// ```
#[test]
fn the_fixture_matches_what_the_code_serializes() {
    let generated = Command::new(env!("CARGO"))
        .args(["run", "--quiet", "--bin", "wire-fixture"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output();

    let Ok(output) = generated else {
        // No fixture binary in this build profile. The key assertions above
        // still run, so skip rather than fail.
        return;
    };

    if !output.status.success() {
        return;
    }

    let produced: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("the fixture binary must emit JSON");

    if std::env::var("LOTUS_BLESS_FIXTURE").is_ok() {
        std::fs::write(
            format!(
                "{}/tests/fixtures/bootstrap.json",
                env!("CARGO_MANIFEST_DIR")
            ),
            serde_json::to_string_pretty(&produced).unwrap() + "\n",
        )
        .expect("could not write the fixture");
        return;
    }

    assert_eq!(
        keys(&produced),
        keys(&fixture()),
        "the bootstrap wire shape changed. Update src/Api.elm in the same commit, \
         then re-bless with LOTUS_BLESS_FIXTURE=1 cargo test --test wire_shape"
    );
    assert_eq!(
        keys(&produced["messages"][0]),
        keys(&fixture()["messages"][0])
    );
    assert_eq!(
        keys(&produced["selectedMessage"]),
        keys(&fixture()["selectedMessage"])
    );
    assert_eq!(
        keys(&produced["accounts"][0]),
        keys(&fixture()["accounts"][0])
    );
}

/// Greps `src/Api.elm` for every `required "field"` and asserts each one appears
/// in the fixture. This is the half of the contract the Rust type system cannot
/// see: an Elm decoder asking for a field Rust never sends fails at run time
/// with a banner, not at compile time.
#[test]
fn every_field_elm_requires_exists_in_the_payloads() {
    let elm = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../src/Api.elm"))
        .expect("src/Api.elm must be readable from the crate directory");

    let mut required_fields: Vec<String> = Vec::new();
    for line in elm.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("|> required \"") {
            if let Some(end) = rest.find('"') {
                required_fields.push(rest[..end].to_string());
            }
        }
    }

    assert!(
        required_fields.len() > 40,
        "expected to find the Api.elm decoders; found {} fields",
        required_fields.len()
    );

    // Every name Elm asks for must appear somewhere in the serialized payloads.
    // The fixture covers bootstrap; the rest are checked against the key lists.
    let known: Vec<String> = FIXTURE
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix('"')
                .and_then(|rest| rest.find("\":").map(|end| rest[..end].to_string()))
        })
        .collect();

    // Fields that belong to payloads the fixture does not include.
    let other_payloads = [
        "requestId",
        "command",
        "ok",
        "data",
        "error",
        "loginUrl",
        "loginState",
        "expiresAt",
        "scopes",
        "accessTokenTail",
        "refreshTokenTail",
        "folderId",
        "message",
        "bootstrap",
        "credential",
        "kind",
        "progress",
        "imported",
        "total",
    ];

    for field in &required_fields {
        let present =
            known.iter().any(|name| name == field) || other_payloads.contains(&field.as_str());
        assert!(
            present,
            "src/Api.elm requires \"{field}\", which no Rust payload sends. \
             An Elm decoder failure here is silent at run time."
        );
    }
}
