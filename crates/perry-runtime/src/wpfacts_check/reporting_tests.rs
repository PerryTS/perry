//! Subprocess controls exercise reporting without inventing solver claims.
use super::*;
use std::process::Command;

const SITE: &str = r#"{"marker":"PERRY_FALSE_PROOF","module":"<runtime-unit>","byte_offset":0,"key":"<unit>","verdict":"Unit","proven_receivers":[],"copy":{"body":"reporting_failure_child","lowered":{"body":"reporting_failure_child","instruction":0}},"compile_sha":"unit-control""#;
const INLINE_SITE: &str = r#"{"marker":"PERRY_FALSE_PROOF","module":"<runtime-unit>","byte_offset":0,"key":"<unit>","verdict":"Unit","proven_receivers":[],"copy":{"body":"reporting_failure_child","lowered":{"body":"reporting_failure_child","instruction":0}},"compile_sha":"unit-control","predicate":"inline check""#;

extern "C" fn two_parameters(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    _first: f64,
    _second: f64,
) -> f64 {
    panic!("a reporting control must not execute the body")
}

static TWO_PARAMETERS: crate::closure::JsFunctionInfo = crate::closure::JsFunctionInfo::of(
    two_parameters as crate::codegen_abi::JsBody2<crate::closure::ClosureHeader>,
)
.plain();

#[test]
fn reporting_failure_child() {
    let Ok(case) = std::env::var("PERRY_VERIFY_FACTS_TEST_CHILD") else {
        return;
    };
    let text = if case == "inline" { INLINE_SITE } else { SITE };
    let (site, len) = (text.as_ptr() as i64, text.len() as i64);
    match case.as_str() {
        "tag" => {
            perry_verify_facts_call(7.0, 0, site, len);
        }
        "arity" => {
            let closure = crate::closure::js_closure_alloc(&TWO_PARAMETERS, 0);
            let value = f64::from_bits(POINTER_TAG | closure as u64);
            perry_verify_facts_call(value, 1, site, len);
        }
        "body" => {
            perry_verify_facts_call_miss(&TWO_PARAMETERS, site, len);
        }
        "inline" => {
            perry_verify_facts_miss(7.0, site, len);
        }
        _ => panic!("unknown reporting control"),
    }
    panic!("a failed claim must end the child process")
}

fn assert_report(case: &str, predicate: &str) {
    // Cargo's abort-test wrapper converts abnormal statuses to 101; observe the child directly.
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "-Z",
            "unstable-options",
            "--force-run-in-process",
            "--exact",
            "wpfacts_check::reporting_tests::reporting_failure_child",
            "--nocapture",
        ])
        .env("PERRY_VERIFY_FACTS_TEST_CHILD", case)
        .env_remove("PERRY_CONTRACTS_REPORT")
        .env_remove("__RUST_TEST_INVOKE")
        .output()
        .unwrap();
    #[cfg(unix)]
    let status = {
        use std::os::unix::process::ExitStatusExt;
        child
            .status
            .code()
            .unwrap_or_else(|| 128 + child.status.signal().unwrap())
    };
    #[cfg(not(unix))]
    let status = child.status.code().unwrap_or(-1);
    let stderr = String::from_utf8(child.stderr).unwrap();
    assert_eq!(status, 86, "{case}: {stderr}");
    assert_eq!(stderr.lines().count(), 1, "{case}: {stderr}");
    let report: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    for field in [
        "marker",
        "module",
        "byte_offset",
        "key",
        "verdict",
        "predicate",
        "proven_receivers",
        "observed_receiver",
        "observed_member",
        "observed_callee",
        "copy",
        "compile_sha",
    ] {
        assert!(report.get(field).is_some(), "{case}: missing {field}");
    }
    assert_eq!(report["marker"], "PERRY_FALSE_PROOF");
    assert_eq!(report["predicate"], predicate);
    assert_eq!(report["module"], "<runtime-unit>");
    assert_eq!(report["key"], "<unit>");
    assert_eq!(report["compile_sha"], "unit-control");
    assert_eq!(report["copy"]["lowered"]["body"], "reporting_failure_child");
}

#[test]
fn invalid_callee_reports_json() {
    assert_report("tag", "callee set");
}
#[test]
fn wrong_plain_arity_reports_json() {
    assert_report("arity", "call form");
}
#[test]
fn wrong_body_info_reports_json() {
    assert_report("body", "callee set");
}
#[test]
fn inline_miss_reports_json() {
    assert_report("inline", "inline check");
}
