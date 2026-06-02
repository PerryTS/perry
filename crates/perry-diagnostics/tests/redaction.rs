//! Mandatory table-driven redaction tests for #849 compatibility reports.
//!
//! The compatibility-report channel is opt-in but the privacy invariant is
//! non-negotiable: nothing that looks like raw source — quoted strings,
//! template literals, user-named identifiers, numeric literals — may
//! survive redaction. These tests pin that behaviour against a curated
//! corpus so we notice the day someone "improves" the redactor and
//! accidentally lets a string through.
//!
//! Two layers:
//!
//! 1. `redact_corpus` — table of `(name, input, expected_substrings_present,
//!    expected_substrings_absent)`. Each case must:
//!    a. redact successfully,
//!    b. produce the expected placeholders,
//!    c. NOT contain any of the forbidden substrings (the raw identifiers,
//!       string contents, etc.).
//!
//! 2. `reject_corpus` — table of inputs that should already be rejected
//!    (defence in depth: even if someone wires a future variant that
//!    skips redaction, building a `CompatibilityReport` would still fail).

use perry_diagnostics::compat_report::{
    redact_snippet, CompatibilityReport, RedactionError, ReportCategory, ReportStage,
    MAX_SNIPPET_LEN,
};
use perry_diagnostics::DiagnosticCode;

struct RedactCase {
    name: &'static str,
    input: &'static str,
    /// Substrings that MUST appear in the redacted output.
    must_contain: &'static [&'static str],
    /// Substrings that MUST NOT appear in the redacted output (would leak
    /// the user's source).
    must_not_contain: &'static [&'static str],
}

const REDACT_CASES: &[RedactCase] = &[
    RedactCase {
        name: "double-quoted string is replaced",
        input: r#"const greeting = "hello world";"#,
        must_contain: &["\"<str>\"", "<id1>"],
        must_not_contain: &["hello", "world", "greeting"],
    },
    RedactCase {
        name: "single-quoted string is replaced",
        input: "let secret = 'sk_live_abc123';",
        must_contain: &["\"<str>\"", "<id1>"],
        must_not_contain: &["sk_live", "abc123", "secret"],
    },
    RedactCase {
        name: "template literal is replaced",
        input: "const msg = `hello ${userName}, your token is ${token}`;",
        must_contain: &["`<tpl>`", "<id1>"],
        must_not_contain: &["hello", "userName", "token", "msg"],
    },
    RedactCase {
        name: "numeric literals are replaced",
        input: "const x = 42 + 3.14 * 0xff;",
        must_contain: &["<num>", "<id1>"],
        must_not_contain: &["42", "3.14", "0xff"],
    },
    RedactCase {
        name: "preserved builtin identifiers survive verbatim",
        input: "console.log(Math.max(99, 88));",
        must_contain: &["console", "Math", "<num>"],
        // `1`/`2` would false-match inside `<id1>`/`<id2>`, so test with
        // distinctive numbers that can't appear in any placeholder.
        must_not_contain: &["99", "88"],
    },
    RedactCase {
        name: "Promise + await preserved, user names anonymized",
        input: "async function fetchUser() { return await Promise.all([fetch(url)]); }",
        must_contain: &["Promise", "await", "fetch", "<id1>"],
        must_not_contain: &["fetchUser"],
    },
    RedactCase {
        name: "identical idents get the same number",
        input: "let foo = foo + foo;",
        must_contain: &["<id1>"],
        must_not_contain: &["<id2>", "foo"],
    },
    RedactCase {
        name: "two different idents get different numbers",
        input: "let foo = bar;",
        must_contain: &["<id1>", "<id2>"],
        must_not_contain: &["foo", "bar"],
    },
    RedactCase {
        name: "node:* module name in a string is redacted (we don't pass it through)",
        input: "import { createHook } from 'node:async_hooks';",
        must_contain: &["import", "\"<str>\"", "<id1>"],
        must_not_contain: &["async_hooks", "createHook"],
    },
    RedactCase {
        name: "decorator survives as a punctuation-prefixed id",
        input: "@inject() class UserService {}",
        must_contain: &["class", "<id1>", "<id2>"],
        must_not_contain: &["inject", "UserService"],
    },
    RedactCase {
        name: "line comment is stripped",
        input: "let x = 1; // user-private comment with email@example.com",
        must_contain: &["<id1>", "<num>"],
        must_not_contain: &["user-private", "email", "example.com"],
    },
    RedactCase {
        name: "block comment is stripped",
        input: "/* TODO: hide this */ let secret = 'x';",
        must_contain: &["<id1>", "\"<str>\""],
        must_not_contain: &["TODO", "hide", "secret"],
    },
    RedactCase {
        name: "escaped quotes inside string don't break redaction",
        input: r#"let a = "she said \"hi\" yesterday";"#,
        must_contain: &["<id1>", "\"<str>\""],
        must_not_contain: &["she", "hi", "yesterday"],
    },
    RedactCase {
        name: "no leaked literal between two strings",
        input: r#"const a = "first" + leakedVar + "second";"#,
        must_contain: &["<id1>", "<id2>", "\"<str>\""],
        must_not_contain: &["first", "second", "leakedVar"],
    },
    RedactCase {
        name: "BigInt literal handled",
        input: "let n = 9007199254740993n;",
        must_contain: &["<num>", "<id1>"],
        must_not_contain: &["9007199254740993"],
    },
    RedactCase {
        name: "exponent notation literal handled",
        input: "let n = 1.5e10;",
        must_contain: &["<num>", "<id1>"],
        must_not_contain: &["1.5", "e10"],
    },
    RedactCase {
        name: "underscore numeric separator handled",
        input: "let n = 1_000_000;",
        must_contain: &["<num>", "<id1>"],
        must_not_contain: &["1_000_000", "1000000"],
    },
    RedactCase {
        name: "preserved process/require survive",
        input: "const fs = require('fs');",
        must_contain: &["require", "\"<str>\"", "<id1>"],
        must_not_contain: &["fs ", " fs"],
    },
    RedactCase {
        name: "long input is truncated to <= MAX_SNIPPET_LEN with ellipsis",
        input: "let a = 1; let b = 2; let c = 3; let d = 4; let e = 5; let f = 6; let g = 7; let h = 8; let i = 9; let j = 10; let k = 11; let l = 12; let m = 13; let n = 14; let o = 15; let p = 16; let q = 17; let r = 18; let s = 19; let t = 20; let u = 21; let v = 22; let w = 23;",
        must_contain: &["..."],
        must_not_contain: &[], // length checked below by name
    },
    RedactCase {
        name: "whitespace collapsed",
        input: "let     x    =     1;",
        must_contain: &["<id1> = <num>"],
        must_not_contain: &["    "],
    },
    RedactCase {
        name: "keyword-only snippet preserved",
        input: "for (let i = 0; i < n; i++) {}",
        must_contain: &["for", "let", "<num>", "<id1>"],
        must_not_contain: &["0", "n;"], // 'n;' is a substring of "n;" which the original has — verifies identifier renaming
    },
    RedactCase {
        name: "unsupported binary op snippet",
        input: "let mask = a ?? b;",
        must_contain: &["??", "<id1>", "<id2>"],
        must_not_contain: &["mask", "a ", "b;"],
    },
];

#[test]
fn redact_corpus() {
    for case in REDACT_CASES {
        let result = redact_snippet(case.input);
        let redacted = match result {
            Ok(r) => r,
            Err(e) => panic!(
                "case `{}` should redact but failed with {:?}\ninput: {:?}",
                case.name, e, case.input
            ),
        };

        // Length cap
        assert!(
            redacted.chars().count() <= MAX_SNIPPET_LEN,
            "case `{}` exceeded MAX_SNIPPET_LEN ({} > {}): {:?}",
            case.name,
            redacted.chars().count(),
            MAX_SNIPPET_LEN,
            redacted
        );

        for s in case.must_contain {
            assert!(
                redacted.contains(s),
                "case `{}` missing required placeholder {:?}\nredacted: {:?}",
                case.name,
                s,
                redacted
            );
        }
        for s in case.must_not_contain {
            assert!(
                !redacted.contains(s),
                "case `{}` leaked forbidden substring {:?}\nredacted: {:?}",
                case.name,
                s,
                redacted
            );
        }
    }
}

#[test]
fn redact_long_input_truncates_with_ellipsis() {
    // 300 distinct keyword tokens (each token is the preserved keyword
    // `let` — preserved idents survive verbatim, so the redacted output
    // stays long enough to exceed MAX_SNIPPET_LEN and trigger truncation).
    let long: String = std::iter::repeat("let ").take(80).collect();
    let r = redact_snippet(&long).unwrap();
    assert!(
        r.chars().count() <= MAX_SNIPPET_LEN,
        "got {} chars, max {}",
        r.chars().count(),
        MAX_SNIPPET_LEN
    );
    assert!(r.ends_with("..."), "expected ellipsis, got {:?}", r);
}

#[test]
fn redact_empty_input_is_rejected() {
    assert!(matches!(
        redact_snippet(""),
        Err(RedactionError::EmptyAfterRedaction)
    ));
    assert!(matches!(
        redact_snippet("   \n\t  "),
        Err(RedactionError::EmptyAfterRedaction)
    ));
}

#[test]
fn build_report_rejects_when_redaction_would_fail_on_empty() {
    let r = CompatibilityReport::build(
        DiagnosticCode::UnsupportedExpression,
        ReportCategory::GapCategorical,
        ReportStage::HirLower,
        "",
        "client".into(),
        "0.0.0".into(),
        "darwin-arm64".into(),
        "20".into(),
        None,
        None,
    );
    assert!(matches!(r, Err(RedactionError::EmptyAfterRedaction)));
}

#[test]
fn build_report_succeeds_with_clean_snippet() {
    let r = CompatibilityReport::build(
        DiagnosticCode::UnsupportedExpression,
        ReportCategory::GapCategorical,
        ReportStage::HirLower,
        "const x = await foo();",
        "client-uuid".into(),
        "0.5.1000".into(),
        "darwin-arm64".into(),
        "20".into(),
        Some("await-in-non-async".into()),
        None,
    )
    .expect("clean snippet must redact");
    assert_eq!(r.code, "UnsupportedExpression");
    assert_eq!(r.category, "gap-categorical");
    assert_eq!(r.stage, "hir-lower");
    assert!(r.snippet_hash.starts_with("sha256:"));
    assert_eq!(r.snippet_hash.len(), "sha256:".len() + 64);
    assert!(!r.snippet_redacted.contains("foo"));
    assert!(r.snippet_redacted.contains("await"));
    // Serializes
    let json = serde_json::to_string(&r).unwrap();
    assert!(json.contains("\"perry_version\":\"0.5.1000\""));
    assert!(!json.contains("foo"));
}

#[test]
fn deterministic_hash_for_same_redacted_input() {
    // Two different raw inputs that redact to the same shape must produce
    // the same snippet_hash, so server-side dedup works.
    let a = CompatibilityReport::build(
        DiagnosticCode::UnsupportedExpression,
        ReportCategory::GapCategorical,
        ReportStage::HirLower,
        "const userA = doStuff();",
        "c".into(),
        "0".into(),
        "linux".into(),
        "20".into(),
        None,
        None,
    )
    .unwrap();
    let b = CompatibilityReport::build(
        DiagnosticCode::UnsupportedExpression,
        ReportCategory::GapCategorical,
        ReportStage::HirLower,
        "const userB = doStuff();",
        "c".into(),
        "0".into(),
        "linux".into(),
        "20".into(),
        None,
        None,
    )
    .unwrap();
    // Identifiers anonymize to <id1>/<id2> in the same positions, so the
    // redacted snippets — and therefore the hashes — are identical.
    assert_eq!(a.snippet_hash, b.snippet_hash);
}
