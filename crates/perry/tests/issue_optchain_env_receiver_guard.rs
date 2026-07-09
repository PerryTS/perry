//! An optional method call whose receiver is an inline `process.env` read —
//! `process.env?.[key]?.trim()` / `process.env.KEY?.trim()` — dropped its
//! per-receiver null-guard and called the method on the `undefined` an unset
//! variable reads as. It returned the STRING `"undefined"` (from stringifying
//! the missing value) instead of short-circuiting to `undefined`.
//!
//! Root cause: `process.env[k]` lowers to `IndexGet { object: ProcessEnv, .. }`,
//! and `opt_call_receiver_repeatable` did not treat `ProcessEnv`/env reads as
//! repeatable, so the `a?.b?.method()` lowering took its "receiver not safe to
//! duplicate → skip the guard" path. Env reads are pure and idempotent, so
//! they are safe to evaluate twice (guard + call); classifying them repeatable
//! restores the guard.
//!
//! This shape is ubiquitous — SDKs read config via `readEnv(k)?.trim()`; a
//! popular client's base-URL default is
//! `process.env.BASE_URL?.trim() ?? "https://…"`, which silently became the
//! string `"undefined"` and produced `new URL("undefined/…")` failures.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Compile+run `source` with `env` cleared of the probe var names, return trimmed stdout.
fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        // Ensure the probed vars are unset so the reads short-circuit.
        .env_remove("PERRY_TEST_UNSET_A")
        .env_remove("PERRY_TEST_UNSET_B")
        .env_remove("PERRY_TEST_SET_C")
        .env("PERRY_TEST_SET_C", "  hello  ")
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary exited non-zero: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).trim().to_string()
}

#[test]
fn optchain_method_on_inline_process_env_short_circuits() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = compile_and_run(
        dir.path(),
        r#"
// Unset var, computed key, chained method (the base-URL default shape).
const dyn = (k: string) => process.env?.[k]?.trim() ?? "DEFAULT";
// Unset var, static key, chained method.
const stat = process.env.PERRY_TEST_UNSET_B?.trim() ?? "DEFAULT";
// SET var still flows through (proves we didn't just null everything).
const setVal = process.env.PERRY_TEST_SET_C?.trim() ?? "DEFAULT";
// A missing read must be JS `undefined`, not the string "undefined".
const raw = process.env?.["PERRY_TEST_UNSET_A"]?.trim();

process.stdout.write(
  "dyn=" + dyn("PERRY_TEST_UNSET_A") +
  " stat=" + stat +
  " set=" + setVal +
  " rawIsUndef=" + (raw === undefined) +
  " rawType=" + typeof raw + "\n"
);
"#,
    );
    assert_eq!(
        out, "dyn=DEFAULT stat=DEFAULT set=hello rawIsUndef=true rawType=undefined",
        "optional method call on inline process.env read"
    );
}
