#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::process::Command;

fn run_examples(reject_required: bool) -> (std::process::Output, serde_json::Value, String) {
    let dir = tempfile::tempdir().unwrap();
    let examples = dir.path().join("examples");
    std::fs::create_dir(&examples).unwrap();
    std::fs::write(
        examples.join("required.ts"),
        "// requires: auto-optimize\n// run: false\nconsole.log('required');\n",
    )
    .unwrap();
    std::fs::write(
        examples.join("ordinary.ts"),
        "// run: false\nconsole.log('ordinary');\n",
    )
    .unwrap();
    std::fs::write(
        examples.join("z_after.ts"),
        "// run: false\nconsole.log('after');\n",
    )
    .unwrap();
    let compiler = dir.path().join("compiler.sh");
    std::fs::write(
        &compiler,
        r#"#!/bin/sh
case "$1" in
    */required.ts)
        if test "${PERRY_NO_AUTO_OPTIMIZE+x}" = x; then
            echo 'required example inherited PERRY_NO_AUTO_OPTIMIZE' >&2
            exit 7
        fi
        echo required:auto >> "$PERRY_DOC_TEST_COMPILER_LOG"
        if test "$PERRY_DOC_TEST_REJECT_REQUIRED" = 1; then
            echo 'required example compilation failed' >&2
            exit 17
        fi
        ;;
    */ordinary.ts|*/z_after.ts)
        if test "$PERRY_NO_AUTO_OPTIMIZE" != 1; then
            echo 'ordinary example lost its prebuilt-archive setting' >&2
            exit 8
        fi
        echo "$(basename "$1" .ts):prebuilt" >> "$PERRY_DOC_TEST_COMPILER_LOG"
        ;;
    *) exit 9 ;;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(&compiler, std::fs::Permissions::from_mode(0o755)).unwrap();
    let report = dir.path().join("report.json");
    let log = dir.path().join("compiler.log");
    let output = Command::new(env!("CARGO_BIN_EXE_doc-tests"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["--skip-xcompile", "--examples-dir"])
        .arg(&examples)
        .arg("--perry")
        .arg(&compiler)
        .arg("--json")
        .arg(&report)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_DOC_TEST_COMPILER_LOG", &log)
        .env(
            "PERRY_DOC_TEST_REJECT_REQUIRED",
            if reject_required { "1" } else { "0" },
        )
        .output()
        .unwrap();
    let report = serde_json::from_slice(&std::fs::read(report).unwrap()).unwrap();
    (output, report, std::fs::read_to_string(log).unwrap())
}

#[test]
fn required_compilation_removes_only_its_own_no_auto_override() {
    let (output, report, calls) = run_examples(false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(report["passed"], 3);
    assert_eq!(report["failed"], 0);
    assert_eq!(report["skipped"], 0);
    assert_eq!(
        calls,
        "ordinary:prebuilt\nrequired:auto\nz_after:prebuilt\n"
    );
}

#[test]
fn required_compilation_failures_are_counted_and_fail_the_harness() {
    let (output, report, calls) = run_examples(true);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(report["passed"], 2);
    assert_eq!(report["failed"], 1);
    assert_eq!(report["skipped"], 0);
    assert_eq!(report["results"][1]["status"], "compile_fail");
    assert!(report["results"][1]["detail"]
        .as_str()
        .unwrap()
        .contains("exit=17"));
    assert_eq!(
        calls,
        "ordinary:prebuilt\nrequired:auto\nz_after:prebuilt\n"
    );
}
