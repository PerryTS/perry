//! Regression test for #9589: the heap must come DOWN while the program is
//! idle, not freeze at whatever the last allocation-triggered cycle left.
//!
//! The program builds ~300k small objects (enough to run several young
//! collections and promote the survivors), reads `heapUsed`, drops the whole
//! structure, and then does nothing for six seconds behind a single timer.
//! Before the idle-time reclaim existed nothing could collect in that window —
//! every collection was allocation-scheduled and an idle program allocates
//! nothing — so `heapUsed` stayed exactly where the last mid-build cycle left
//! it. This is the shape the compiled claude-code TUI showed at 425–680 MB
//! through minutes of idle.
//!
//! Two arms, so the result is attributable to the mechanism and not to the
//! rebuild: the default arm must reclaim, and must show the reducer's own
//! counters live in `PERRY_GC_DIAG` output; the `PERRY_GC_IDLE_RECLAIM=0` arm
//! must stay flat. The OFF arm is also what CLAUDE.md's knob kill-policy asks
//! for — the kill switch's off state exercised in CI.

use std::path::PathBuf;
use std::process::Command;

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
    "PERRY_GC_IDLE_RECLAIM",
    "PERRY_GC_DIAG",
];

fn remove_gc_env_overrides(command: &mut Command) {
    for key in GC_ENV_OVERRIDES {
        command.env_remove(key);
    }
}

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const FIXTURE: &str = r#"
function fill(): object[] {
    const keep: object[] = [];
    for (let i = 0; i < 300000; i++) {
        keep.push({ a: i, b: [i, i + 1, i + 2], s: "item-" + i });
    }
    return keep;
}
let keep: object[] | null = fill();
const before = process.memoryUsage().heapUsed;
keep = null;
setTimeout(() => {
    const after = process.memoryUsage().heapUsed;
    const verdict = after * 2 < before ? "RECLAIMED" : "FLAT";
    console.log(`${verdict} before=${before} after=${after}`);
}, 6000);
"#;

struct Run {
    stdout: String,
    stderr: String,
}

fn compile_fixture(dir: &std::path::Path) -> PathBuf {
    let entry = dir.join("idle_reclaim.ts");
    let output = dir.join("idle_reclaim_bin");
    std::fs::write(&entry, FIXTURE).expect("write fixture");
    let mut compile = Command::new(perry_bin());
    remove_gc_env_overrides(&mut compile);
    let compile = compile
        .current_dir(dir)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_NO_CACHE", "1")
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
    output
}

fn run_fixture(binary: &std::path::Path, idle_reclaim: Option<&str>) -> Run {
    let mut run = Command::new(binary);
    remove_gc_env_overrides(&mut run);
    run.env("PERRY_GC_DIAG", "1");
    if let Some(value) = idle_reclaim {
        run.env("PERRY_GC_IDLE_RECLAIM", value);
    }
    let out = run.output().expect("run compiled fixture");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        out.status.success(),
        "fixture exited {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status
    );
    Run { stdout, stderr }
}

/// The `[gc-idle-reclaim] ... completions=N ...` exit-line counter.
fn idle_reclaim_completions(stderr: &str) -> Option<u64> {
    stderr
        .lines()
        .filter(|line| line.starts_with("[gc-idle-reclaim] enabled="))
        .find_map(|line| {
            line.split_whitespace()
                .find_map(|field| field.strip_prefix("completions="))
                .and_then(|value| value.parse().ok())
        })
}

#[test]
fn completions_parser_reads_the_exit_line() {
    let stderr = "[gc-idle-reclaim] start attempt=1 external_collections=4 backoff_shift=0 old_in_use=1\n\
                  [gc-idle-reclaim] enabled=true attempts=1 completions=1 productive=1 freed_bytes=9 old_reclaimed_bytes=9 slices=3 yields=0 start_blocked=0 work_capped=0 post_purges=1 backoff_shift=0\n";
    assert_eq!(idle_reclaim_completions(stderr), Some(1));
    assert_eq!(idle_reclaim_completions("[gc-time] wall_us=1\n"), None);
}

#[test]
fn idle_program_reclaims_its_dead_heap_and_the_kill_switch_keeps_it_flat() {
    let dir = tempfile::tempdir().expect("tempdir");
    let binary = compile_fixture(dir.path());

    // Default arm: the reducer runs during the six idle seconds and the dead
    // structure is gone by the time the timer reads heapUsed.
    let on = run_fixture(&binary, None);
    assert!(
        on.stdout.starts_with("RECLAIMED "),
        "default arm must reclaim during idle\nstdout:\n{}\nstderr:\n{}",
        on.stdout,
        on.stderr
    );
    let completions = idle_reclaim_completions(&on.stderr).unwrap_or_else(|| {
        panic!(
            "LIVE SUBJECT: no [gc-idle-reclaim] exit line in PERRY_GC_DIAG output\n{}",
            on.stderr
        )
    });
    assert!(
        completions >= 1,
        "LIVE SUBJECT: the reducer must have completed a full, got {completions}\n{}",
        on.stderr
    );
    assert!(
        on.stderr.contains("[gc-idle-reclaim] start attempt=1"),
        "the reducer's start line must be present\n{}",
        on.stderr
    );

    // Kill-switch arm: same binary, nothing collects while idle, heapUsed is
    // exactly where the last allocation-triggered cycle left it.
    let off = run_fixture(&binary, Some("0"));
    assert!(
        off.stdout.starts_with("FLAT "),
        "PERRY_GC_IDLE_RECLAIM=0 must leave the idle heap untouched\nstdout:\n{}\nstderr:\n{}",
        off.stdout,
        off.stderr
    );
    assert!(
        off.stderr
            .contains("[gc-idle-reclaim] enabled=false attempts=0 completions=0"),
        "OFF arm must report itself off and inert\n{}",
        off.stderr
    );
}
