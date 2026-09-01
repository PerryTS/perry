//! #9342: in-function reads of a module-global / parameter `Uint8Array`.
//!
//! A perry `Uint8Array` is a `BufferHeader` in the buffer registries, invisible
//! to `lookup_typed_array_kind`. Before the fix, two stacked defects:
//!
//!  1. **Wrong answers** — the typed-array checked-load lane admits the class
//!     name `"Uint8Array"` (kind 1), but its `PERRY_TA_KIND_CACHE` guard can
//!     never match a `BufferHeader`, so every read routed to
//!     `js_typed_array_read_f64` / `js_typed_array_read_int32`, whose
//!     registry-miss arms answered `undefined` / `0` for every in-range
//!     element.
//!  2. **12× slowness** — the `Uint8ArrayGet` fallback was a per-element
//!     runtime call feeding a dynamic add (bench_buffer_readwrite's
//!     in-function cliff: 560 ms vs node 38 ms).
//!
//! The fix gives untracked-but-proven u8 receivers their own buffer-lane
//! inline read (`expr/u8_buffer_read.rs` + `PERRY_U8_INLINE_CACHE`), ordered
//! BEFORE the typed-array checked lane. That ordering is a performance trap
//! with no wrong answer — if someone reorders the lanes back, the TA guard
//! captures u8 receivers into permanent slow-helper calls and nothing fails —
//! so `ta_lane_must_not_capture_u8_receivers` pins it structurally: a program
//! whose only array-ish receiver is a u8 buffer must emit NO
//! `js_typed_array_read_f64` reference at all.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Module-global receiver read in a function, plus a typed-parameter receiver:
/// the two shapes the tracked-view fast path cannot serve.
///
/// Expected values (node-checked):
///  * `sum(buf)` over `buf[i] = i % 256` for N=4096: 16 full 0..=255 blocks →
///    16 * 32640 = 522240.
///  * OOB read `buf[4096]` is `undefined`; `undefined + 0` shows as NaN via
///    `Number()` coercion in the harness — asserted as the string "NaN".
///  * A Uint8Array view over a Uint32Array's materialized buffer observes all
///    four backing bytes after the u32 write → 1 + 2 + 3 + 4 = 10.
const SOURCE: &str = r#"
const N = 4096;
const buf = new Uint8Array(N);
for (let i = 0; i < N; i++) buf[i] = i % 256;

function viaGlobal(): number {
    let s = 0;
    for (let i = 0; i < N; i++) s += buf[i];
    return s;
}

function viaParam(b: Uint8Array): number {
    let s = 0;
    for (let i = 0; i < N; i++) s += b[i];
    return s;
}

function oobGlobal(): number {
    // In-bounds proof intentionally absent for index N.
    let x = 0;
    x += buf[N];
    return x;
}

function viaAliasedView(): number {
    const words = new Uint32Array(1);
    const bytes = new Uint8Array(words.buffer);
    words[0] = 0x01020304;
    return bytes[0] + bytes[1] + bytes[2] + bytes[3];
}

console.log(viaGlobal() + "," + viaParam(buf) + "," + oobGlobal() + "," + viaAliasedView());
"#;

const EXPECTED: &str = "522240,522240,NaN,10\n";

fn compile(dir: &Path, source: &str, extra_env: &[(&str, &str)]) -> (PathBuf, String) {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");
    let mut cmd = Command::new(perry_bin());
    cmd.current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_LLVM_KEEP_IR", "1");
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let compile = cmd.output().expect("run perry compile");
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

/// Count CALL sites of `name`, not the `declare` line every module emits for
/// every runtime symbol. Matching the bare name is worthless here: both
/// `js_u8_buffer_read_f64` and `js_typed_array_read_f64` are declared in every
/// compiled module whether or not anything calls them, so a bare-substring
/// absence assertion can never pass and a bare-substring presence assertion
/// passes vacuously. (Both mistakes were live in this file's first draft and
/// were caught by running it.)
fn call_count(ir: &str, name: &str) -> usize {
    let needle = format!("@{name}(");
    ir.lines()
        .filter(|l| l.contains(&needle) && l.contains("call "))
        .count()
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

fn run(bin: &Path, dir: &Path, gc_stress: bool) -> Output {
    let mut command = Command::new(bin);
    command.current_dir(dir);
    if gc_stress {
        command
            .env("PERRY_GC_HEAP_LIMIT", "8")
            .env("PERRY_GC_FORCE_EVACUATE", "1");
    }
    command.output().expect("run compiled binary")
}

fn assert_stdout(output: &Output, label: &str) {
    assert!(
        output.status.success(),
        "{label}: binary failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        EXPECTED,
        "{label}: wrong element values"
    );
}

/// The inline lane fires (guard + slow helper present), values are node-exact,
/// and they stay node-exact under heap-limit + forced-evacuation GC stress
/// (the cache is address-keyed; a stale hit would read recycled memory).
#[test]
fn u8_inline_read_lane_fires_and_is_correct() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, stderr) = compile(dir.path(), SOURCE, &[]);
    let ir = kept_ir(&stderr);
    assert!(
        call_count(&ir, "js_u8_buffer_read_f64") > 0,
        "the u8 inline lane's slow arm must be CALLED — the lane did not fire"
    );
    assert!(
        ir.contains("@PERRY_U8_INLINE_CACHE"),
        "the emitted guard must probe the admission cache"
    );
    assert_stdout(&run(&bin, dir.path(), false), "plain");
    assert_stdout(&run(&bin, dir.path(), true), "gc-stress");
}

/// Ordering pin: the typed-array checked lane must NOT capture a u8 receiver.
/// Its `PERRY_TA_KIND_CACHE` guard can never admit a `BufferHeader`, so
/// capture means every read is a permanent slow-helper call — a performance
/// regression nothing else would ever fail on. This program's only array-ish
/// receiver is a u8 buffer, so a `js_typed_array_read_f64` reference in the
/// IR can only mean the lane ordering regressed
/// (`expr/index_get.rs`: u8 lane before `try_lower_ta_param_f64_read`).
#[test]
fn ta_lane_must_not_capture_u8_receivers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (_bin, stderr) = compile(dir.path(), SOURCE, &[]);
    let ir = kept_ir(&stderr);
    assert_eq!(
        call_count(&ir, "js_typed_array_read_f64"),
        0,
        "a u8 receiver reached the typed-array checked lane — its guard can \
         never admit a BufferHeader, so every read becomes a slow-helper call"
    );
    // Vacuity guard: the absence above is only meaningful while this fixture
    // still admits the u8 lane. If the lane stops firing, the absence passes
    // for the wrong reason.
    assert!(
        call_count(&ir, "js_u8_buffer_read_f64") > 0,
        "fixture no longer admits the u8 lane — the absence assertion above \
         would pass vacuously"
    );
}

/// Kill switch: `PERRY_U8_INLINE_READ=0` at build time removes the lane and
/// the program still answers node-exact values through the runtime helpers
/// (which, post-#9342, recover buffer elements on a registry miss instead of
/// inventing `undefined`/`0`).
#[test]
fn kill_switch_stays_correct() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, stderr) = compile(dir.path(), SOURCE, &[("PERRY_U8_INLINE_READ", "0")]);
    let ir = kept_ir(&stderr);
    assert_eq!(
        call_count(&ir, "js_u8_buffer_read_f64"),
        0,
        "kill switch must remove the inline lane"
    );
    assert_stdout(&run(&bin, dir.path(), false), "kill-switch");
}
