//! `-`, `*` and `/` take a guarded inline path with the dynamic helper in a
//! cold arm (#9157 follow-up).
//!
//! The guard is a runtime tag test, so nothing static is trusted. These tests
//! exist to pin that: every case feeds the "numeric" path something that is
//! not a number — a string laundered through `as any`, a BigInt, `null`,
//! `undefined` — and the answer must still be the one JavaScript specifies.
//!
//! BigInt is the interesting one. It is not a Number, so it fails the guard
//! and lands in the cold arm, which is what makes `10n - 3n` still produce a
//! BigInt and `10n - 3` still throw.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str, guard: bool) -> String {
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
    if !guard {
        cmd.env("PERRY_GUARDED_ARITH", "0");
    }
    let compile = cmd.output().expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed (guard={guard})\nstdout:\n{}\nstderr:\n{}",
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

/// The guard changes speed, never the answer, so both lowerings must agree.
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
fn numeric_operands_still_compute() {
    assert_same_both_ways(
        "numeric",
        r#"
        const arr: number[] = [];
        for (let i = 0; i < 10; i++) arr.push(1);
        const v = arr.length;
        console.log(String(v - 3) + " " + String(v * 3) + " " + String(v / 4));
        "#,
        "7 30 2.5",
    );
}

#[test]
fn a_lying_number_annotation_still_coerces() {
    assert_same_both_ways(
        "lying annotation",
        r#"
        const liar: any = "10";
        const n: number = liar;
        console.log(String(n - 3) + " " + String(n * 2) + " " + String(n / 2));
        "#,
        "7 20 5",
    );
}

#[test]
fn bigint_operands_take_the_cold_arm() {
    // A BigInt is not a Number, so it fails the guard — which is exactly what
    // keeps BigInt arithmetic working through a numeric-looking fast path.
    assert_same_both_ways(
        "bigint",
        r#"
        const b1: any = 10n, b2: any = 3n;
        console.log(String(b1 - b2) + " " + String(b1 * b2) + " " + String(b1 / b2));
        "#,
        "7 30 3",
    );
}

#[test]
fn mixing_bigint_and_number_still_throws() {
    assert_same_both_ways(
        "bigint mixed",
        r#"
        const b: any = 10n;
        try { console.log(String(b - 3)); } catch (e) { console.log((e as Error).constructor.name); }
        "#,
        "TypeError",
    );
}

#[test]
fn the_operand_cross_product_follows_js_rules() {
    assert_same_both_ways(
        "cross product",
        r#"
        const vals: any[] = [1, "8", 2.5, true, null, undefined, "x"];
        let out = "";
        for (let i = 0; i < vals.length; i++) {
            for (let j = 0; j < vals.length; j++) {
                out += String(vals[i] - vals[j]) + "," + String(vals[i] * vals[j]) + "," + String(vals[i] / vals[j]) + ";";
            }
        }
        console.log(out.length + " " + out.slice(0, 40));
        "#,
        "530 0,1,1;-7,8,0.125;-1.5,2.5,0.4;0,1,1;1,0,",
    );
}

#[test]
fn signed_zero_and_infinity_edges_survive() {
    assert_same_both_ways(
        "edges",
        r#"
        console.log(String(0 - 0) + " " + String(-0 * 1) + " " + String(1 / 0) + " " + String(-1 / 0) + " " + String(0 / 0) + " " + String(1e308 * 10));
        "#,
        "0 0 Infinity -Infinity NaN Infinity",
    );
}

#[test]
fn an_operand_that_turns_non_numeric_midway_switches_arms() {
    assert_same_both_ways(
        "arm switches",
        r#"
        const step: any[] = [2, 4, "x", 5];
        let acc: any = 100;
        let out = "";
        for (let i = 0; i < step.length; i++) { acc = acc - step[i]; out += String(acc) + ";"; }
        console.log(out);
        "#,
        "98;94;NaN;NaN;",
    );
}
