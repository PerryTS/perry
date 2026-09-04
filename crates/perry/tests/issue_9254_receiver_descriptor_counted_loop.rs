//! Regression coverage for #9254 phase 3: ordinary `i < arr.length` loops can
//! validate a numeric receiver once, then carry its refreshed address through
//! loop polls instead of repeating the full structural guard at every read.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile(dir: &Path, source: &str) -> (PathBuf, String) {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_LLVM_KEEP_IR", "1")
        .env_remove("PERRY_GC_MOVING_LOOP_POLLS")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    (
        output,
        String::from_utf8_lossy(&compile.stderr).into_owned(),
    )
}

fn kept_ir(stderr: &str) -> String {
    let path = stderr
        .lines()
        .find_map(|line| line.split("kept LLVM IR: ").nth(1))
        .map(str::trim)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("PERRY_LLVM_KEEP_IR did not report an IR path\n{stderr}"));
    std::fs::read_to_string(path).expect("read kept LLVM IR")
}

fn run(bin: &Path, dir: &Path, scheduled_gc: bool) -> Output {
    let mut command = Command::new(bin);
    command.current_dir(dir);
    for key in [
        "PERRY_GC_SCHEDULE_SEED",
        "PERRY_GC_SCHEDULE_RATE",
        "PERRY_GC_SCHEDULE_ALLOC_KB",
        "PERRY_GC_FORCE_EVACUATE",
        "PERRY_GC_VERIFY_EVACUATION",
        "PERRY_GC_PROTECT_FROMSPACE",
        "PERRY_GC_PROTECT_FROMSPACE_DEPTH",
    ] {
        command.env_remove(key);
    }
    if scheduled_gc {
        command
            .env("PERRY_GC_SCHEDULE_SEED", "9254")
            .env("PERRY_GC_SCHEDULE_RATE", "1")
            .env("PERRY_GC_SCHEDULE_ALLOC_KB", "0")
            .env("PERRY_GC_FORCE_EVACUATE", "1")
            .env("PERRY_GC_VERIFY_EVACUATION", "1")
            .env("PERRY_GC_PROTECT_FROMSPACE", "1")
            .env("PERRY_GC_PROTECT_FROMSPACE_DEPTH", "64");
    }
    command.output().expect("run compiled binary")
}

fn verdict_field(stderr: &str, field: &str) -> u64 {
    let verdict = stderr
        .lines()
        .rev()
        .find(|line| line.contains("[gc-schedule] forced_collections="))
        .unwrap_or_else(|| panic!("scheduled run emitted no exercise verdict\n{stderr}"));
    verdict
        .split_ascii_whitespace()
        .find_map(|part| part.strip_prefix(&format!("{field}=")))
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| panic!("scheduled verdict has no numeric {field}\n{verdict}"))
}

const VALUES: &str = "[0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5, 8.5, 9.5, 10.5, 11.5, 12.5, \
     13.5, 14.5, 15.5, 16.5, 17.5, 18.5, 19.5, 20.5, 21.5, 22.5, 23.5, 24.5, \
     25.5, 26.5, 27.5, 28.5, 29.5, 30.5, 31.5]";

fn admitted_source() -> &'static str {
    include_str!("../../../test-files/test_issue_9254_receiver_descriptor_counted_loop.ts")
}

#[test]
fn ordinary_counted_loop_consumes_the_descriptor_and_refreshes_it_at_real_polls() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, compile_stderr) = compile(dir.path(), admitted_source());
    let ir = kept_ir(&compile_stderr);

    let fast_start = ir
        .find("\nbidx.num.receiver_region.fast")
        .unwrap_or_else(|| panic!("the ordinary bounded read did not consume a descriptor\n{ir}"));
    let fast_tail = &ir[fast_start + 1..];
    let fast_end = fast_tail
        .find("\nbidx.num.receiver_region.fallback")
        .expect("receiver-region fallback block follows its fast block");
    let fast_block = &fast_tail[..fast_end];
    assert!(
        fast_block.contains("load i64") && fast_block.contains("load double"),
        "descriptor fast block must load the refreshed handle and raw element\n{fast_block}"
    );
    assert!(
        !fast_block.contains("call "),
        "the descriptor fast block repeated a runtime guard/call\n{fast_block}"
    );
    assert!(
        ir.contains("call i32 @js_typed_feedback_numeric_array_index_get_guard")
            && ir.contains("i32 0, i32 0"),
        "receiver-only numeric validation was not emitted in the preheader\n{ir}"
    );
    assert!(
        ir.contains("call void @js_gc_loop_safepoint"),
        "the subject has no real poll and cannot prove refresh safety\n{ir}"
    );

    let plain = run(&bin, dir.path(), false);
    assert!(
        plain.status.success(),
        "plain run failed\nstderr:\n{}",
        String::from_utf8_lossy(&plain.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&plain.stdout), "32\n0\n");

    let scheduled = run(&bin, dir.path(), true);
    assert!(
        scheduled.status.success(),
        "scheduled moving-GC run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&scheduled.stdout),
        String::from_utf8_lossy(&scheduled.stderr)
    );
    assert_eq!(scheduled.stdout, plain.stdout);
    let scheduled_stderr = String::from_utf8_lossy(&scheduled.stderr);
    for field in ["copying_minors", "moved_objects", "loop_polls"] {
        assert!(
            verdict_field(&scheduled_stderr, field) > 0,
            "scheduled run did not exercise {field}\n{scheduled_stderr}"
        );
    }
}

#[test]
fn allocating_region_keeps_the_per_read_guard() {
    let source = format!(
        r#"
function sum(a: number[]): number {{
  let total = 0;
  for (let i = 0; i < a.length; i++) {{
    const scratch = {{ value: i }};
    if (scratch.value < -1) total = total + 1000000;
    total = total + a[i];
  }}
  return total;
}}
const values: number[] = {VALUES};
console.log(sum(values));
"#
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let (_, compile_stderr) = compile(dir.path(), &source);
    let ir = kept_ir(&compile_stderr);
    assert!(
        !ir.contains("receiver_region.fast"),
        "an allocation/user-code-capable region incorrectly carried the descriptor\n{ir}"
    );
    assert!(
        ir.contains("bidx.num.guard.deref")
            || ir.contains("call i32 @js_typed_feedback_numeric_array_index_get_guard"),
        "negative control did not retain the established per-read guard\n{ir}"
    );
}
