//! The GC environment-knob parse contract, pinned in **both** directions
//! (#7991).
//!
//! The bug this closes: `gc_diag_enabled()` read `PERRY_GC_DIAG` with
//! `var_os(..).is_some()` — *presence*, not value — so `PERRY_GC_DIAG=0`
//! turned diagnostics ON, as did `off`, `false` and the empty string. That is
//! a measurement-integrity bug, not a cosmetic one: during #7803 triage it
//! silently collapsed an A/B arm, because the investigator's "diagnostics off"
//! control arm got exactly the same diagnostics as the instrumented arm. The
//! failure direction is the dangerous one — a confident wrong answer rather
//! than a visible error.
//!
//! It was also *inconsistent with its immediate neighbours*:
//! `PERRY_GC_PROTECT_FROMSPACE` two modules over has parsed its value properly
//! all along, so `=0` really was off there. Two adjacent GC knobs with
//! opposite conventions is the drift CLAUDE.md's knob kill-policy exists to
//! prevent, so this file asserts the *shared vocabulary* rather than one
//! knob's behaviour — a future knob that hand-rolls its own `matches!` is the
//! regression, and `scripts/check_gc_env_knobs.py` rejects the presence-only
//! shape outright.
//!
//! Everything here is a pure function of the raw value. That is deliberate:
//! the live readers cache in a `OnceLock`, and `std::env::set_var` is
//! process-wide, so a test that set the real variable would be at the mercy of
//! which libtest thread ran first (`knob_overrides` in `gc/mod.rs` records
//! what that cost us — 5 failures in 100 runs across three unrelated cases).

use super::super::policy::{safepoint_only_contract_from_value, SafepointOnlyContract};
use super::super::{env_default_on_from_value, env_flag_from_value};

/// Every spelling a human might reasonably use to mean "off", plus the two
/// that actually bit: `Some("0")` and `Some("")`.
const OFF_SPELLINGS: &[Option<&str>] = &[
    None,
    Some("0"),
    Some("off"),
    Some("false"),
    Some("no"),
    Some(""),
    Some("   "),
    Some("OFF"),
    Some("False"),
    Some(" 0 "),
];

const ON_SPELLINGS: &[&str] = &["1", "true", "on", "yes", "TRUE", "On", " 1 ", "YES"];

/// The unrecognised case gets its own name because it is the one an
/// unthinking `!is_off()` implementation gets wrong: a typo must leave a
/// default-OFF instrument OFF, not arm it.
const UNRECOGNISED: &[&str] = &["banana", "2", "-1", "onn", "ye", "enabled", "0x1"];

#[test]
fn default_off_knobs_are_parsed_by_value_not_presence() {
    for raw in OFF_SPELLINGS {
        assert!(
            !env_flag_from_value(*raw),
            "{raw:?} must read as OFF — presence is not consent. `PERRY_GC_DIAG=0` \
             enabling diagnostics is exactly how #7803's control arm was lost."
        );
    }
    for raw in ON_SPELLINGS {
        assert!(env_flag_from_value(Some(raw)), "{raw:?} must read as ON");
    }
    for raw in UNRECOGNISED {
        assert!(
            !env_flag_from_value(Some(raw)),
            "{raw:?} is unrecognised and must leave a default-OFF knob OFF, not \
             silently arm it"
        );
    }
}

/// The mirror contract. A default-ON kill switch must fail toward its own
/// default, so it is **not** the negation of the default-OFF parser: an
/// unrecognised value leaves the shipping feature ON.
#[test]
fn default_on_kill_switches_only_fire_on_an_explicit_off() {
    assert!(
        env_default_on_from_value(None),
        "unset must leave a default-ON feature ON"
    );
    for raw in ["0", "off", "false", "no", "OFF", "False", " 0 "] {
        assert!(
            !env_default_on_from_value(Some(raw)),
            "{raw:?} must disable a default-ON kill switch"
        );
    }
    for raw in ["1", "true", "on", "yes", ""] {
        assert!(
            env_default_on_from_value(Some(raw)),
            "{raw:?} must leave a default-ON feature ON"
        );
    }
    for raw in UNRECOGNISED {
        assert!(
            env_default_on_from_value(Some(raw)),
            "{raw:?} is unrecognised; a typo must not silently disable a shipping \
             collector default"
        );
    }
}

/// The two parsers are not each other's negation, and that asymmetry is the
/// point rather than an oversight — so it gets an assertion of its own, or a
/// future tidy-up will "simplify" one into the other.
#[test]
fn the_two_vocabularies_disagree_only_on_the_unrecognised_case() {
    for raw in UNRECOGNISED {
        assert!(!env_flag_from_value(Some(raw)));
        assert!(env_default_on_from_value(Some(raw)));
    }
    // ...and agree everywhere a value is recognised.
    for raw in ["1", "true", "on", "yes"] {
        assert!(env_flag_from_value(Some(raw)) && env_default_on_from_value(Some(raw)));
    }
    for raw in ["0", "off", "false", "no"] {
        assert!(!env_flag_from_value(Some(raw)) && !env_default_on_from_value(Some(raw)));
    }
}

/// `PERRY_GC_SAFEPOINT_ONLY` is three-state. Its boolean arm must share the one
/// vocabulary; only `strict` is its own.
#[test]
fn safepoint_only_is_three_state_over_the_shared_vocabulary() {
    for raw in OFF_SPELLINGS {
        assert_eq!(
            safepoint_only_contract_from_value(*raw),
            SafepointOnlyContract::Off,
            "{raw:?} must leave the safepoint-only contract Off"
        );
    }
    for raw in ON_SPELLINGS {
        assert_eq!(
            safepoint_only_contract_from_value(Some(raw)),
            SafepointOnlyContract::Heal,
            "{raw:?} must select Heal"
        );
    }
    for raw in ["strict", "STRICT", " strict "] {
        assert_eq!(
            safepoint_only_contract_from_value(Some(raw)),
            SafepointOnlyContract::Strict,
            "{raw:?} must select Strict"
        );
    }
    for raw in UNRECOGNISED {
        assert_eq!(
            safepoint_only_contract_from_value(Some(raw)),
            SafepointOnlyContract::Off,
            "{raw:?} is unrecognised and must not arm a contract enforcer"
        );
    }
}

/// The decisive arm: the **live cached reader**, initialised in a child
/// process under a real `PERRY_GC_DIAG=0`.
///
/// The pure cases above pin the vocabulary; they do not by themselves prove
/// `gc_diag_enabled()` uses it — reverting that one line to
/// `var_os(..).is_some()` leaves every one of them green. Nothing short of
/// observing the shipping reader under the shipping environment closes that,
/// and it cannot be done in-process: the reader caches in a `OnceLock` and
/// `set_var` is visible to every other libtest thread, so an in-process ON arm
/// would be both racy and order-dependent. A child process is the isolation.
///
/// The child re-runs *this* test with the marker set, so the assertion and the
/// environment that produces it stay in one place.
#[test]
fn perry_gc_diag_zero_really_disables_diagnostics() {
    // Deliberately NOT a `PERRY_GC_*` name: this is test-harness plumbing, and
    // the GC knob family is audited (`scripts/check_gc_env_knobs.py`).
    const CHILD_ENV: &str = "PERRY_TEST_GC_DIAG_PARSE_CHILD";
    if let Some(expected) = std::env::var_os(CHILD_ENV) {
        let want = expected == *"on";
        assert_eq!(
            super::super::gc_diag_enabled(),
            want,
            "PERRY_GC_DIAG={:?} must read as {}",
            std::env::var_os("PERRY_GC_DIAG"),
            if want { "ON" } else { "OFF" }
        );
        return;
    }

    // (raw value, expected verdict). `0` is the case that shipped broken; the
    // ON arm is here so a fix that hard-wires `false` cannot pass either — a
    // liveness counter satisfiable by two paths is a presence check, not a
    // proof.
    for (raw, want_on) in [("0", false), ("off", false), ("", false), ("1", true)] {
        let status =
            std::process::Command::new(std::env::current_exe().expect("current test binary"))
                .arg("gc::tests::env_knob_parse::perry_gc_diag_zero_really_disables_diagnostics")
                .arg("--exact")
                .arg("--nocapture")
                .env("PERRY_GC_DIAG", raw)
                .env(CHILD_ENV, if want_on { "on" } else { "off" })
                .status()
                .expect("launch isolated PERRY_GC_DIAG witness");
        assert!(
            status.success(),
            "PERRY_GC_DIAG={raw:?} did not read as {}; presence is not consent",
            if want_on { "ON" } else { "OFF" }
        );
    }
}

/// The remaining boolean GC knobs, asserted OFF when unset — the state every CI
/// job and every developer shell is in, and what makes an un-instrumented run
/// trustworthy.
#[test]
fn the_live_readers_are_off_when_their_knobs_are_unset() {
    // If any of these were still presence-parsed they would still be off here
    // (the vars are unset), so this is a *default* assertion, not a proof of
    // the parse — that is what the pure cases above are for. It is worth
    // having anyway: the default-off contract for a diagnostic knob is what
    // makes an un-instrumented run trustworthy.
    for name in [
        "PERRY_GC_DIAG",
        "PERRY_GC_TRACE",
        "PERRY_GC_VERIFY_MARK",
        "PERRY_GC_VERIFY_EVACUATION",
        "PERRY_GC_VERIFY_RS_NONFATAL",
        "PERRY_GC_VERIFY_CLASSIFIER",
        "PERRY_GC_FORCE_EVACUATE",
    ] {
        if std::env::var_os(name).is_some() {
            // An operator running the suite under a knob is not a test
            // failure; skip rather than assert something untrue.
            continue;
        }
        assert!(
            !super::super::env_flag_enabled(name),
            "{name} is unset and must read as OFF"
        );
    }
}
