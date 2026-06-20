//! Regression test for #5459 — use-after-free when `arr.push(...)` targets a
//! module-level array from inside a nested function (closure/IIFE).
//!
//! Root cause: in `lower/expr/array_push.rs`, the `boxed_vars` write-back branch
//! returned early after handling the captured/local-box cases. A module-level
//! global that is in `boxed_vars` (because a nested function references it) but
//! has NO box location in the callee context falls through both inner arms — so
//! the early return skipped the realloc write-back entirely. When `js_array_push_f64`
//! relocated the array head, the new head was never stored back to the
//! GC-root global slot: the old head was freed on the next GC and the global
//! dangled (garbage `.length`, freed elements → SIGSEGV under churn).
//!
//! The fix only returns early when a box location was actually written;
//! otherwise it falls through to the module-global store-back
//! (`emit_root_nanbox_store_on_block`). Same fix for `ArrayPushSpread`.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, entry: &std::path::Path) -> (bool, String) {
    let output = dir.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(entry)
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
    let run = Command::new(&output).output().expect("run compiled binary");
    (
        run.status.success(),
        String::from_utf8_lossy(&run.stdout).to_string(),
    )
}

/// A module-level array populated from inside an IIFE must survive allocation
/// churn + gc() with all elements intact (pre-fix: SIGSEGV / garbage length).
#[test]
fn module_global_array_pushed_in_iife_survives_gc() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
declare function gc(): void;
function churn(n: number): void {
  let j: any[] = [];
  for (let i = 0; i < n; i++) { j.push({ i, p: "z".repeat(40) + i }); if (j.length > 128) j = []; }
}
const strong: any[] = [];
(function setup() { for (let n = 0; n < 50; n++) strong.push({ id: n }); })();
for (let c = 0; c < 12; c++) { churn(80000); gc(); }
if ((strong.length as number) !== 50) { console.log("BADLEN " + strong.length); }
let alive = 0;
for (let n = 0; n < 50; n++) if (strong[n] && strong[n].id === n) alive++;
console.log("survived " + alive);
"#,
    )
    .expect("write entry");

    let (ok, stdout) = compile_and_run(dir.path(), &entry);
    assert!(
        ok,
        "binary crashed (use-after-free regression)\nstdout:\n{stdout}"
    );
    assert!(
        stdout.contains("survived 50"),
        "all 50 module-global array elements must survive GC (got: {stdout})"
    );
    assert!(
        !stdout.contains("BADLEN"),
        "module-global array length must stay 50 across GC (got: {stdout})"
    );
}

/// Same hazard via spread-push (`arr.push(...xs)`) into a module-global from a
/// nested function.
#[test]
fn module_global_array_spread_pushed_in_iife_survives_gc() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
declare function gc(): void;
function churn(n: number): void {
  let j: any[] = [];
  for (let i = 0; i < n; i++) { j.push({ i, p: "z".repeat(40) + i }); if (j.length > 128) j = []; }
}
const acc: any[] = [];
(function setup() { for (let n = 0; n < 25; n++) acc.push(...[{ id: n }, { id: n + 100 }]); })();
for (let c = 0; c < 12; c++) { churn(80000); gc(); }
let ok = 0;
for (let n = 0; n < acc.length; n++) if (acc[n] && typeof acc[n].id === "number") ok++;
console.log("spread " + ok + "/" + acc.length);
"#,
    )
    .expect("write entry");

    let (ok, stdout) = compile_and_run(dir.path(), &entry);
    assert!(
        ok,
        "binary crashed (spread use-after-free regression)\nstdout:\n{stdout}"
    );
    assert!(
        stdout.contains("spread 50/50"),
        "all 50 spread-pushed elements must survive GC (got: {stdout})"
    );
}
