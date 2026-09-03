//! Live subject for #9644: a fragmented old generation must come back to the
//! OS while the program is idle.
//!
//! The idle reclaim (#9589) sweeps at idle, which finds the dead old-gen
//! objects and puts them on the old-gen free list. It cannot hand them back:
//! the free list lives inside partially-occupied blocks, and a non-moving
//! sweep never empties one. Measured on the compiled claude-code TUI, 50 MB of
//! a 93.6 MB old gen sat exactly there, across two reducer fulls, with the
//! live-block count unchanged at 130.
//!
//! The fixture reproduces that shape deliberately — 400k old objects, three of
//! every four dropped, so every old page keeps a live occupant — and then
//! idles. It is also the fragmentation corpus #7917 asked for and did not
//! have: no benchmark program in the suite can produce a page where
//! `dead_bytes >= live_bytes`, which is why old-page defrag has had a benefit
//! signal of exactly zero.
//!
//! Two arms: the default one must run a compaction that releases block-granule
//! memory, and `PERRY_GC_IDLE_COMPACT=0` must report itself off and inert.

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
    "PERRY_GC_IDLE_COMPACT",
    "PERRY_GC_OLD_DEFRAG",
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

/// 400k objects promoted together, then three of every four dropped: the
/// survivors are spread over every page they shared, so no page is all-dead
/// and no block can be released without moving something.
const FIXTURE: &str = r#"
const N = 400000;
const keep: (object | null)[] = [];
for (let i = 0; i < N; i++) {
    keep.push({ a: i, b: "item-" + i, c: [i, i + 1] });
}
for (let i = 0; i < N; i++) {
    if (i % 4 !== 0) {
        keep[i] = null;
    }
}
const before = process.memoryUsage().heapUsed;
setTimeout(() => {
    let live = 0;
    for (let i = 0; i < N; i += 4) {
        if (keep[i] !== null) {
            live++;
        }
    }
    const after = process.memoryUsage().heapUsed;
    console.log(`DONE before=${before} after=${after} live=${live}`);
}, 10000);
"#;

struct Run {
    stdout: String,
    stderr: String,
}

fn compile_fixture(dir: &std::path::Path) -> PathBuf {
    let entry = dir.join("idle_compact.ts");
    let output = dir.join("idle_compact_bin");
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

fn run_fixture(binary: &std::path::Path, idle_compact: Option<&str>) -> Run {
    let mut run = Command::new(binary);
    remove_gc_env_overrides(&mut run);
    run.env("PERRY_GC_DIAG", "1");
    if let Some(value) = idle_compact {
        run.env("PERRY_GC_IDLE_COMPACT", value);
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

/// `released_bytes=N` off the `[gc-idle-compact] enabled=...` exit line.
fn released_bytes(stderr: &str) -> Option<u64> {
    stderr
        .lines()
        .filter(|line| line.starts_with("[gc-idle-compact] enabled="))
        .find_map(|line| {
            line.split_whitespace()
                .find_map(|field| field.strip_prefix("released_bytes="))
                .and_then(|value| value.parse().ok())
        })
}

#[test]
fn released_bytes_parser_reads_the_exit_line() {
    let stderr = "[gc-idle-compact] enabled=true attempts=1 productive=1 released_bytes=8388608 \
                  pause_us_total=41000 pause_us_max=41000 wake_declined=0 backoff_shift=0\n";
    assert_eq!(released_bytes(stderr), Some(8_388_608));
    assert_eq!(released_bytes("[gc-time] wall_us=1\n"), None);
}

#[test]
fn a_fragmented_idle_heap_gives_its_blocks_back_and_the_kill_switch_keeps_them() {
    let dir = tempfile::tempdir().expect("tempdir");
    let binary = compile_fixture(dir.path());

    let on = run_fixture(&binary, None);
    assert!(
        on.stdout.starts_with("DONE "),
        "fixture must reach its timer\nstdout:\n{}\nstderr:\n{}",
        on.stdout,
        on.stderr
    );
    assert!(
        on.stderr.contains("[gc-idle-compact] start attempt=1"),
        "LIVE SUBJECT: no compaction was attempted on a heap built to need one\n{}",
        on.stderr
    );
    let released = released_bytes(&on.stderr).unwrap_or_else(|| {
        panic!(
            "LIVE SUBJECT: no [gc-idle-compact] exit line in PERRY_GC_DIAG output\n{}",
            on.stderr
        )
    });
    assert!(
        released > 0,
        "the compaction must have released block-granule memory, got {released}\n{}",
        on.stderr
    );

    let off = run_fixture(&binary, Some("0"));
    assert!(
        off.stderr
            .contains("[gc-idle-compact] enabled=false attempts=0"),
        "OFF arm must report itself off and inert\n{}",
        off.stderr
    );
    assert_eq!(
        released_bytes(&off.stderr),
        Some(0),
        "nothing may be released with the kill switch on\n{}",
        off.stderr
    );
}
