//! Subprocess death tests for the A1 fail-closed aborts (design 2.4).
//!
//! A death test re-executes this test binary on one test with the child
//! marker set: the child performs the fatal action, the parent asserts that it
//! died and what it said. [`assert_child_aborts`] must itself be able to fail,
//! which `death_helper_fails_when_the_child_survives` proves.

use super::*;

const CHILD_MARKER: &str = "PERRY_A1_DEATH_CHILD";

/// True in the re-executed child of a death test.
pub(crate) fn in_death_child() -> bool {
    std::env::var_os(CHILD_MARKER).is_some()
}

/// Run `test` (an exact test name) in a child of this binary and assert that
/// it was killed without exiting normally and that its stderr contains
/// `needle`. Returns the child's stderr.
pub(crate) fn assert_child_aborts(test: &str, needle: &str) -> String {
    let child = std::process::Command::new(std::env::current_exe().expect("current test exe"))
        .args(["--exact", test, "--nocapture", "--test-threads", "1"])
        .env(CHILD_MARKER, "1")
        .output()
        .expect("spawn death-test child");
    let stdout = String::from_utf8_lossy(&child.stdout);
    let stderr = String::from_utf8_lossy(&child.stderr).into_owned();
    assert!(
        stdout.contains("running 1 test"),
        "the death-test child did not run {test}: {stdout}"
    );
    assert!(
        !child.status.success(),
        "{test} was expected to abort but exited with {:?}",
        child.status
    );
    assert!(
        stderr.contains(needle),
        "{test} aborted without `{needle}`: {stderr}"
    );
    stderr
}

/// Child body for the helper's own test: returns normally.
#[test]
fn death_child_that_survives() {
    if !in_death_child() {
        return;
    }
    // Falls through: a child that does NOT abort.
}

/// The helper must fail on a child that does not abort.
#[test]
fn death_helper_fails_when_the_child_survives() {
    if in_death_child() {
        return;
    }
    let outcome = std::panic::catch_unwind(|| {
        assert_child_aborts("object::a1_death_tests::death_child_that_survives", "");
    });
    assert!(
        outcome.is_err(),
        "assert_child_aborts accepted a child that did not abort"
    );
}

/// Child body: the A1 internal error on an ordinary object.
#[test]
fn a1_internal_error_child() {
    if !in_death_child() {
        return;
    }
    crate::test_support::bootstrap_test_thread();
    unsafe {
        let obj = js_object_alloc(7, 1);
        a1_internal_error(obj, MetaReason::Spill);
    }
}

#[test]
fn a1_internal_error_aborts_naming_the_family_and_the_reason() {
    if in_death_child() {
        return;
    }
    assert_child_aborts(
        "object::a1_death_tests::a1_internal_error_child",
        "perry internal error A1: meta-less family",
    );
    assert_child_aborts(
        "object::a1_death_tests::a1_internal_error_child",
        "reached Spill",
    );
}
