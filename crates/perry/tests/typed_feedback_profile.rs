//! #8504: exercise real capture/replay, stale-input isolation and JS parity.
#![cfg(unix)]
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

const SOURCE: &str = include_str!("../../../test-files/test_typed_feedback_profile_replay.ts");

fn success(output: Output) -> Output {
    assert!(
        output.status.success(),
        "status={}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
fn compile(dir: &Path, name: &str, args: &[&str], instrument: bool) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_perry"));
    cmd.current_dir(dir)
        .args(["compile", "main.ts", "-o", name, "--no-cache"])
        .args(args)
        .env_remove("PERRY_TYPED_FEEDBACK")
        .env_remove("PERRY_TYPED_FEEDBACK_TRACE");
    if instrument {
        cmd.env("PERRY_TYPED_FEEDBACK", "1");
    }
    cmd.output().unwrap()
}
fn run(dir: &Path, name: &str, disagree: bool, trace: Option<&Path>) -> Output {
    let mut cmd = Command::new(dir.join(name));
    cmd.current_dir(dir)
        .env_remove("PERRY_TYPED_FEEDBACK")
        .env_remove("PERRY_TYPED_FEEDBACK_TRACE");
    if disagree {
        cmd.arg("disagree");
    }
    if let Some(trace) = trace {
        cmd.env("PERRY_TYPED_FEEDBACK_TRACE", trace);
    }
    success(cmd.output().unwrap())
}
fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn capture_replay_guard_failure_and_semantic_parity() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    std::fs::write(dir.join("main.ts"), SOURCE).unwrap();
    success(compile(
        dir,
        "capture",
        &["--typed-feedback-sites", "sites.json"],
        true,
    ));
    let trace_path = dir.join("capture-trace.json");
    let captured = run(dir, "capture", false, Some(&trace_path));
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/typed-feedback-profile.py");
    success(
        Command::new("python3")
            .arg(&script)
            .current_dir(dir)
            .args([
                "--sites",
                "sites.json",
                "--trace",
                "capture-trace.json",
                "-o",
                "profile.json",
            ])
            .output()
            .unwrap(),
    );
    // Conversion is deterministic too, independent of output path.
    success(
        Command::new("python3")
            .arg(script)
            .current_dir(dir)
            .args([
                "--sites",
                "sites.json",
                "--trace",
                "capture-trace.json",
                "-o",
                "profile2.json",
            ])
            .output()
            .unwrap(),
    );
    assert_eq!(
        std::fs::read(dir.join("profile.json")).unwrap(),
        std::fs::read(dir.join("profile2.json")).unwrap()
    );
    let replay_compile = success(compile(
        dir,
        "replay",
        &[
            "--typed-feedback-profile",
            "profile.json",
            "--explain-lowering",
        ],
        true,
    ));
    let stderr = String::from_utf8_lossy(&replay_compile.stderr);
    assert!(
        stderr.contains("[typed-feedback-replay] accepted"),
        "{stderr}"
    );
    assert!(stderr.contains("fresh_numeric_array_observation"));
    assert_eq!(captured.stdout, run(dir, "replay", false, None).stdout);
    let disagree_trace = dir.join("disagree-trace.json");
    let replay = run(dir, "replay", true, Some(&disagree_trace));
    let trace = read_json(&disagree_trace);
    let guarded: Vec<_> = trace["sites"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["guard_name"] == "numeric_array_index_get_guard")
        .collect();
    assert!(
        guarded
            .iter()
            .any(|s| s["guard_failures"].as_u64().unwrap_or(0) > 0
                && s["fallback_calls"].as_u64().unwrap_or(0) > 0),
        "{trace}"
    );
    success(compile(dir, "baseline", &[], false));
    assert_eq!(run(dir, "baseline", true, None).stdout, replay.stdout);
    success(compile(
        dir,
        "replay-normal",
        &[
            "--typed-feedback-profile",
            "profile.json",
            "--verify-native-regions",
        ],
        false,
    ));
    assert_eq!(run(dir, "replay-normal", true, None).stdout, replay.stdout);
    // Node sees the exact same JS after stripping these three TS annotations.
    let js = SOURCE
        .replace(": any[]", "")
        .replace(": any", "")
        .replace(": number", "");
    std::fs::write(dir.join("main.js"), js).unwrap();
    let node = success(
        Command::new("node")
            .current_dir(dir)
            .args(["main.js", "disagree"])
            .output()
            .unwrap(),
    );
    assert_eq!(node.stdout, replay.stdout);
    let mut stale_profile = read_json(&dir.join("profile.json"));
    for module in stale_profile["modules"].as_array_mut().unwrap() {
        module["identity"]["source_hash"] = Value::String("stale".into());
    }
    std::fs::write(
        dir.join("stale.json"),
        serde_json::to_vec(&stale_profile).unwrap(),
    )
    .unwrap();
    let stale_compile = success(compile(
        dir,
        "stale.o",
        &[
            "--typed-feedback-profile",
            "stale.json",
            "--explain-lowering",
            "--no-link",
        ],
        false,
    ));
    let stale_stderr = String::from_utf8_lossy(&stale_compile.stderr);
    assert!(
        stale_stderr.contains("source_hash_mismatch"),
        "{stale_stderr}"
    );
    assert!(!stale_stderr.contains("[typed-feedback-replay] accepted"));
    let reports: Vec<_> = std::fs::read_dir(dir.join(".perry-trace/lowering"))
        .unwrap()
        .map(|e| read_json(&e.unwrap().path().join("explain-lowering.json")))
        .collect();
    assert!(reports
        .iter()
        .any(|r| r["summary"]["typed_path_selection_reason_counts"]
            ["typed_feedback_replay:fresh_numeric_array_observation"]
            .as_u64()
            .unwrap_or(0)
            > 0));
    assert!(reports
        .iter()
        .any(|r| r["summary"]["typed_path_rejection_reason_counts"]
            ["typed_feedback_replay:source_hash_mismatch"]
            .as_u64()
            .unwrap_or(0)
            > 0));
}

#[test]
fn explicit_malformed_profile_has_actionable_diagnostic() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("main.ts"), "console.log(1)").unwrap();
    for invalid in ["{", "{}", "{\"schema_version\":\"one\"}"] {
        std::fs::write(temp.path().join("bad.json"), invalid).unwrap();
        let result = compile(
            temp.path(),
            "unused",
            &["--typed-feedback-profile", "bad.json"],
            false,
        );
        assert!(!result.status.success());
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(
            stderr.contains("invalid --typed-feedback-profile"),
            "{stderr}"
        );
        assert!(
            stderr.contains("scripts/typed-feedback-profile.py"),
            "{stderr}"
        );
    }
    std::fs::write(
        temp.path().join("future.json"),
        r#"{"schema_version": 2, "future_schema_body": []}"#,
    )
    .unwrap();
    let future = success(compile(
        temp.path(),
        "future.o",
        &["--typed-feedback-profile", "future.json", "--no-link"],
        false,
    ));
    assert!(String::from_utf8_lossy(&future.stderr).contains("schema_mismatch"));
    let result = compile(
        temp.path(),
        "unused",
        &["--typed-feedback-profile", "missing.json"],
        false,
    );
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("cannot read --typed-feedback-profile")
    );
}
