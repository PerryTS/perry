//! S2 of the deferred-collection RFC: a fast/slow split may leave only the
//! FAST call without a statepoint. Its slow arm runs user code — a getter, a
//! Proxy trap, a setter, a `toString`, a not-callable throw — and that code
//! allocates enough to move the caller's live heap values. If the slow call
//! were a GC leaf as well, RS4GC would record no stack map at it, the walker
//! would skip the caller's frame, and the caller's values would still name
//! from-space when it reads them afterwards.
//!
//! The fixture is `test-files/test_gap_ic_fast_split_slow_path.ts` (also a gap
//! test, against node). Here it is compiled with `PERRY_FULL_OUTLINE_IC=1`,
//! where the three IC splits apply, and run under a seeded evacuating schedule
//! with the from-space quarantine and the evacuation verifier on. The run must
//! print node's output AND report copying minors that moved objects — a green
//! run with zero moves would say nothing.
//!
//! Sabotage (run for the PR, 2026-09-27): classifying the three `_fast_miss`
//! continuations `CannotCollect` makes this test fail.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const SOURCE: &str = include_str!("../../../test-files/test_gap_ic_fast_split_slow_path.ts");

/// node 26.5.1 (`.node-version`) on `SOURCE`.
const NODE_ORACLE: &str = "keep-g:123:1\nkeep-g:123:1\nkeep-g:123:2\nkeep-g:123:3\nkeep-g:123:1\n\
keep-cg7:10\nkeep-cg7:10\nkeep-cg7:20\nkeep-cg7:10\n\
keep-cs45 11\nkeep-cs45 21\nkeep-cs45 12\n\
keep-t6:s\nkeep-t6:42\nkeep-t6:T\nkeep-t6:s2\n\
no-throw:keep-c\ncaught:true:keep-c89\nno-throw:keep-c\n";

const GC_ENV: &[&str] = &[
    "PERRY_GEN_GC",
    "PERRY_GC_MOVING_LOOP_POLLS",
    "PERRY_GC_SCHEDULE_SEED",
    "PERRY_GC_SCHEDULE_RATE",
    "PERRY_GC_SCHEDULE_ALLOC_KB",
    "PERRY_GC_FORCE_EVACUATE",
    "PERRY_GC_VERIFY_EVACUATION",
    "PERRY_GC_PROTECT_FROMSPACE",
    "PERRY_GC_PROTECT_FROMSPACE_DEPTH",
    "PERRY_CONSERVATIVE_STACK_SCAN",
];

fn compile(dir: &Path) -> (PathBuf, String) {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, SOURCE).expect("write entry");
    let mut cmd = Command::new(perry_bin());
    cmd.current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_GC_INSTRUMENTS", "1")
        .env("PERRY_FULL_OUTLINE_IC", "1")
        .env("PERRY_LLVM_KEEP_IR", "1");
    for key in GC_ENV {
        cmd.env_remove(key);
    }
    let compile = cmd.output().expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    (output, String::from_utf8_lossy(&compile.stderr).into_owned())
}

fn kept_ir(stderr: &str) -> String {
    stderr
        .lines()
        .filter_map(|line| line.split("kept LLVM IR: ").nth(1))
        .map(|p| std::fs::read_to_string(p.trim()).expect("read kept LLVM IR"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn run(bin: &Path, dir: &Path, scheduled: bool) -> Output {
    let mut cmd = Command::new(bin);
    cmd.current_dir(dir);
    for key in GC_ENV {
        cmd.env_remove(key);
    }
    if scheduled {
        cmd.env("PERRY_GC_SCHEDULE_SEED", "11528")
            .env("PERRY_GC_SCHEDULE_RATE", "1")
            .env("PERRY_GC_FORCE_EVACUATE", "1")
            .env("PERRY_GC_VERIFY_EVACUATION", "1")
            .env("PERRY_GC_PROTECT_FROMSPACE", "1")
            .env("PERRY_GC_PROTECT_FROMSPACE_DEPTH", "64");
    }
    cmd.output().expect("run compiled binary")
}

fn verdict_field(stderr: &str, field: &str) -> u64 {
    stderr
        .lines()
        .rev()
        .filter(|line| line.contains("[gc-schedule]"))
        .find_map(|line| {
            line.split_ascii_whitespace()
                .find_map(|part| part.strip_prefix(&format!("{field}=")))
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or_else(|| panic!("scheduled run reported no numeric {field}\n{stderr}"))
}

#[test]
fn slow_arms_that_run_allocating_user_code_keep_the_callers_values_relocated() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, compile_stderr) = compile(dir.path());

    // The subject must be in the artifact: every split, both halves.
    let ir = kept_ir(&compile_stderr);
    for symbol in [
        "@js_object_get_field_ic_fast(",
        "@js_object_get_field_ic_fast_miss(",
        "@js_class_field_get_ic_fast(",
        "@js_class_field_get_ic_fast_miss(",
        "@js_class_field_set_ic_fast(",
        "@js_class_field_set_ic_fast_miss(",
        "@js_template_string_coerce_box(",
        "@js_closure_unbox_callee_checked(",
    ] {
        assert!(
            ir.lines()
                .any(|l| l.contains(symbol) && l.contains("call ") && !l.contains("declare ")),
            "the fixture no longer exercises `{symbol}`"
        );
    }

    let plain = run(&bin, dir.path(), false);
    assert!(
        plain.status.success(),
        "plain run failed\nstderr:\n{}",
        String::from_utf8_lossy(&plain.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&plain.stdout), NODE_ORACLE);

    let scheduled = run(&bin, dir.path(), true);
    let stderr = String::from_utf8_lossy(&scheduled.stderr);
    assert!(
        scheduled.status.success(),
        "evacuating run failed (exit {:?})\nstdout:\n{}\nstderr:\n{stderr}",
        scheduled.status.code(),
        String::from_utf8_lossy(&scheduled.stdout)
    );
    assert_eq!(String::from_utf8_lossy(&scheduled.stdout), NODE_ORACLE);
    for field in ["copying_minors", "moved_objects"] {
        assert!(
            verdict_field(&stderr, field) > 0,
            "the evacuating run never exercised {field}: a green run proves nothing\n{stderr}"
        );
    }
}
