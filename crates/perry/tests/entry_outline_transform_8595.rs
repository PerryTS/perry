//! #8595 entry-outlining transform — end-to-end differential.
//!
//! Oversized entries are outlined automatically; `PERRY_OUTLINE_ENTRY=1`
//! forces the transform on small differential fixtures. Original declarations
//! move unchanged into chunk functions, while module-global discovery gives
//! them shared rooted storage. This must not change observable behavior —
//! including under a relocating minor.
//!
//! The same program is compiled twice from identical source:
//!   * `PERRY_OUTLINE_ENTRY=0` — the ordinary single-function entry;
//!   * `PERRY_OUTLINE_ENTRY=1 PERRY_OUTLINE_ENTRY_CHUNK_STMTS=1` — maximum
//!     chunking, so every top-level statement is its own chunk function and the
//!     object lets `a`/`b`/`c` are genuinely defined in one chunk and read in
//!     another.
//! Both run under every moving-collector arm and must produce byte-identical,
//! correct output. If a cross-chunk global were not rooted/rewritten by a
//! relocating minor, only the outlined arm would diverge.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Exported immutable bindings exercise the module-global/export scans that
/// used to gate outlining out entirely. `a`/`b`/`c` are heap objects defined
/// in separate chunks and read together in a later chunk.
const SOURCE: &str = r#"
let a = { v: 3 };
let b = { v: 4 };
let c = { v: 5 };
export const sum = a.v + b.v + c.v;
console.log("sum:" + sum);
"#;

const EXPECTED: &str = "sum:12\n";

const GC_ENV_OVERRIDES: &[&str] = &[
    "PERRY_GEN_GC",
    "PERRY_GC_SCAVENGE",
    "PERRY_GC_SCAVENGE_NURSERY_MB",
    "PERRY_GC_MOVING_SAFEPOINT",
    "PERRY_GC_MOVING_LOOP_POLLS",
    "PERRY_GC_FORCE_EVACUATE",
    "PERRY_CONSERVATIVE_STACK_SCAN",
    "PERRY_WRITE_BARRIERS",
    "PERRY_GC_INCREMENTAL",
    "PERRY_GC_HEAP_LIMIT",
    // Outlining knobs — cleared so an exported value can't perturb an arm.
    "PERRY_OUTLINE_ENTRY",
    "PERRY_OUTLINE_ENTRY_CHUNK_STMTS",
    "PERRY_OUTLINE_ENTRY_REPORT",
    "PERRY_OUTLINE_SCAN_8595",
];

fn compile(dir: &std::path::Path, name: &str, source: &str, outline: bool) -> (PathBuf, String) {
    let entry = dir.join(format!("{name}.ts"));
    let output = dir.join(name);
    std::fs::write(&entry, source).expect("write entry");
    let mut cmd = Command::new(perry_bin());
    cmd.current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache");
    for key in GC_ENV_OVERRIDES {
        cmd.env_remove(key);
    }
    if outline {
        cmd.env("PERRY_OUTLINE_ENTRY", "1")
            .env("PERRY_OUTLINE_ENTRY_CHUNK_STMTS", "1")
            .env("RUST_LOG", "debug");
    } else {
        cmd.env("PERRY_OUTLINE_ENTRY", "0");
    }
    let out = cmd.output().expect("run perry compile");
    assert!(
        out.status.success(),
        "compile (outline={outline}) failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    (output, String::from_utf8_lossy(&out.stderr).into_owned())
}

fn run_arms(binary: &std::path::Path, dir: &std::path::Path, label: &str, expected: &str) {
    let mut arms: Vec<Vec<(&str, &str)>> = vec![vec![]];
    for mb in ["1", "2", "4"] {
        arms.push(vec![("PERRY_GC_SCAVENGE_NURSERY_MB", mb)]);
    }
    arms.push(vec![("PERRY_GEN_GC", "0")]);
    for arm in &arms {
        let mut cmd = Command::new(binary);
        cmd.current_dir(dir);
        for key in GC_ENV_OVERRIDES {
            cmd.env_remove(key);
        }
        for (k, v) in arm {
            cmd.env(k, v);
        }
        let run = cmd.output().expect("run compiled binary");
        let arm_label = if arm.is_empty() {
            format!("{label}/default")
        } else {
            format!("{label}/{}={}", arm[0].0, arm[0].1)
        };
        assert!(
            run.status.success(),
            "[{arm_label}] binary failed (exit {:?})\nstderr:\n{}",
            run.status.code(),
            String::from_utf8_lossy(&run.stderr),
        );
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            expected,
            "[{arm_label}] wrong output"
        );
        assert!(
            run.stderr.is_empty(),
            "[{arm_label}] unexpected stderr:\n{}",
            String::from_utf8_lossy(&run.stderr)
        );
    }
}

#[test]
fn outlined_entry_matches_the_single_function_entry_under_a_relocating_minor() {
    let dir = tempfile::tempdir().expect("tempdir");

    let (off_bin, _) = compile(dir.path(), "toy_off", SOURCE, false);
    let (on_bin, on_stderr) = compile(dir.path(), "toy_on", SOURCE, true);

    // The outlined compile must have actually outlined (into multiple chunk
    // functions), or the differential proves nothing. The transform logs the
    // count at debug level.
    assert!(
        on_stderr.contains("outlined entry body of 'toy_on.ts' into")
            && on_stderr.contains("chunk functions"),
        "PERRY_OUTLINE_ENTRY=1 was expected to outline the entry, but the \
         compile did not report it:\nstderr:\n{on_stderr}"
    );

    run_arms(&off_bin, dir.path(), "single-function", EXPECTED);
    run_arms(&on_bin, dir.path(), "outlined", EXPECTED);
}

/// Script-global function reflection and a `globalThis` read used to be a
/// conservative coupling bail. Structured control flow now moves as one chunk
/// statement, while the reflection still happens before user code.
const INTERLEAVE_SOURCE: &str = r#"
function reflected() { return 7; }
let a = { v: 10 };
let b = { v: 20 };
if (a.v < b.v) { console.log("less"); }
let c = { v: 30 };
console.log("total:" + (a.v + b.v + c.v + globalThis.reflected()));
"#;

const INTERLEAVE_EXPECTED: &str = "less\ntotal:67\n";

#[test]
fn outlining_preserves_structured_control_flow_and_script_global_reflection() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (off_bin, _) = compile(dir.path(), "int_off", INTERLEAVE_SOURCE, false);
    let (on_bin, on_stderr) = compile(dir.path(), "int_on", INTERLEAVE_SOURCE, true);
    assert!(
        on_stderr.contains("outlined entry body of 'int_on.ts' into")
            && on_stderr.contains("chunk functions"),
        "the structured body must still outline:\nstderr:\n{on_stderr}"
    );
    run_arms(&off_bin, dir.path(), "single-function", INTERLEAVE_EXPECTED);
    run_arms(&on_bin, dir.path(), "outlined", INTERLEAVE_EXPECTED);
}

/// `process.env` literals in the entry are applied before static dependencies
/// initialize. The early scan must follow chunk calls after outlining.
#[test]
fn outlining_keeps_early_process_env_assignment_visible_to_dependencies() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("dep.ts"),
        r#"export const observed = process.env.PERRY_OUTLINE_SCAN_8595 || "missing";"#,
    )
    .expect("write dependency");
    let source = r#"
process.env.PERRY_OUTLINE_SCAN_8595 = "visible";
import { observed } from "./dep";
console.log(observed);
"#;
    let (off_bin, _) = compile(dir.path(), "env_off", source, false);
    let (on_bin, on_stderr) = compile(dir.path(), "env_on", source, true);
    assert!(
        on_stderr.contains("outlined entry body of 'env_on.ts' into"),
        "the env fixture must outline:\nstderr:\n{on_stderr}"
    );
    run_arms(&off_bin, dir.path(), "single-function", "visible\n");
    run_arms(&on_bin, dir.path(), "outlined", "visible\n");
}

#[test]
fn oversized_entry_outlines_automatically_without_an_environment_opt_in() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut source = String::new();
    for id in 0..1_001 {
        source.push_str(&format!("const v{id} = {id};\n"));
    }
    source.push_str("console.log(v0 + v1000);\n");

    let entry = dir.path().join("auto.ts");
    let output = dir.path().join("auto");
    std::fs::write(&entry, source).expect("write automatic outlining fixture");
    let mut cmd = Command::new(perry_bin());
    cmd.current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("RUST_LOG", "debug");
    for key in GC_ENV_OVERRIDES {
        cmd.env_remove(key);
    }
    let compiled = cmd.output().expect("compile automatic outlining fixture");
    assert!(
        compiled.status.success(),
        "automatic outlining compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compiled.stdout),
        String::from_utf8_lossy(&compiled.stderr)
    );
    let stderr = String::from_utf8_lossy(&compiled.stderr);
    assert!(
        stderr.contains("outlined entry body of 'auto.ts' into 6 chunk functions"),
        "the default 1,000-statement gate should emit six bounded chunks:\n{stderr}"
    );

    let run = Command::new(&output)
        .current_dir(dir.path())
        .env_remove("PERRY_OUTLINE_SCAN_8595")
        .output()
        .expect("run automatic outlining fixture");
    assert!(run.status.success(), "automatic outlined binary failed");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "1000\n");
    assert!(run.stderr.is_empty());
}
