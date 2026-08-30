//! Two-leaf dynamic `+` takes the guarded numeric lowering (#9157).
//!
//! `s += v`, where both operands hold numbers at runtime but neither is
//! statically proven, used to lower to `js_dynamic_string_or_number_add` plus
//! a write barrier per add. It now takes the same guarded `fadd` diamond that
//! three-leaf trees already used.
//!
//! The guard is a RUNTIME tag test, so no annotation is trusted — which is the
//! property these tests exist to pin. Every case below feeds the "numeric"
//! path a value whose declared type is a lie, and the answer must still be the
//! one JavaScript specifies: concatenation.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str, pair_guard: bool) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let mut cmd = Command::new(perry_bin());
    cmd.current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_CACHE", "1");
    if !pair_guard {
        cmd.env("PERRY_DYNAMIC_ADD_PAIR_GUARD", "0");
    }
    let compile = cmd.output().expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed (pair_guard={pair_guard})\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).trim().to_owned()
}

/// Both lowerings must agree: the guard changes speed, never the answer.
fn assert_same_both_ways(name: &str, source: &str, expected: &str) {
    assert_eq!(
        compile_and_run(source, true).trim(),
        expected.trim(),
        "{name}: guarded lowering"
    );
    assert_eq!(
        compile_and_run(source, false).trim(),
        expected.trim(),
        "{name}: kill-switch lowering"
    );
}

#[test]
fn numeric_accumulator_still_adds() {
    assert_same_both_ways(
        "numeric",
        r#"
        const arr: number[] = [];
        for (let i = 0; i < 7; i++) arr.push(1);
        const v = arr.length;
        let s = 0;
        for (let i = 0; i < 4; i++) { s += v; }
        console.log(s);
        "#,
        "28",
    );
}

#[test]
fn a_lying_number_annotation_still_concatenates() {
    // The whole point of the guard: the declared type nominates the fast arm,
    // the runtime tag test rejects it, and JS semantics survive.
    assert_same_both_ways(
        "lying annotation",
        r#"
        const liar: any = "5";
        const v: number = liar;
        let s: number = liar;
        s += v;
        console.log(String(s));
        "#,
        "55",
    );
}

#[test]
fn a_lying_parameter_and_field_still_concatenate() {
    assert_same_both_ways(
        "lying param and field",
        r#"
        function add2(x: number, y: number): string { return String(x + y); }
        class Holder { n: number; constructor(n: number) { this.n = n; } }
        const liar: any = "5";
        const h = new Holder(liar);
        console.log(add2(liar, liar) + " " + String(h.n + h.n));
        "#,
        "55 55",
    );
}

#[test]
fn mixed_number_and_string_operands_follow_js_rules() {
    assert_same_both_ways(
        "mixed operands",
        r#"
        const vals: any[] = [1, "a", 2.5, true, null, undefined];
        let out = "";
        for (let i = 0; i < vals.length; i++) {
            for (let j = 0; j < vals.length; j++) {
                out += String(vals[i] + vals[j]) + ";";
            }
        }
        console.log(out);
        "#,
        "2;1a;3.5;2;1;NaN;a1;aa;a2.5;atrue;anull;aundefined;3.5;2.5a;5;3.5;2.5;NaN;2;truea;3.5;2;1;NaN;1;nulla;2.5;1;0;NaN;NaN;undefineda;NaN;NaN;NaN;NaN;",
    );
}

#[test]
fn a_value_that_turns_into_a_string_midway_switches_arms() {
    // Same add site, numeric on the first iterations and string afterwards:
    // the guard is per-execution, so the arm must change with the value.
    assert_same_both_ways(
        "arm switches",
        r#"
        let acc: any = 0;
        const step: any[] = [1, 2, "x", 3];
        let out = "";
        for (let i = 0; i < step.length; i++) { acc += step[i]; out += String(acc) + ";"; }
        console.log(out);
        "#,
        "1;3;3x;3x3;",
    );
}
