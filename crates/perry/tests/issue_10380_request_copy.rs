//! A Request input must survive lowering so construction can inherit its fields.
use std::process::Command;

#[test]
fn request_input_inherits_fields_and_applies_init_overrides() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let entry = dir.path().join("request_copy.ts");
    let binary = dir.path().join("request_copy");
    std::fs::write(
        &entry,
        include_str!("../../../test-files/test_gap_10380_request_copy.ts"),
    )
    .unwrap();
    let compile = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(dir.path())
        .args(["compile", "--no-cache"])
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("compile regression fixture");
    assert!(
        compile.status.success(),
        "compile failed:\n{}\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&binary)
        .output()
        .expect("run regression fixture");
    assert!(
        run.status.success(),
        "run failed:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        concat!(
            "plain POST https://example.test/session payload\n",
            "metadata include no-store manual\n",
            "wrapped POST payload\n",
            "headers Bearer test null\n",
            "override PUT replacement\n",
            "inherited header yes\n",
            "bytes 0 127 255\n",
            "mutated headers updated\n",
            "body transfer true false\n",
            "transferred text payload\n",
            "used body rejected true\n",
            "GET body rejected true\n",
            "null inherits POST payload\n",
            "reflective POST payload yes\n",
            "reflective stream AB\n",
            "subclass PUT derived-body yes\n",
            "subclass transfer true\n",
            "subclass used rejected true\n",
            "text content type text/plain;charset=UTF-8\n",
            "explicit content type application/custom\n",
            "empty content type null\n",
            "handle body constructed POST\n",
            "stream override CD\n",
        )
    );
}
