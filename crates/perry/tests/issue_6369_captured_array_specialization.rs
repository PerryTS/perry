//! Regression test for #6369: a numeric array reached through a *capture* — a
//! module-scope `const` read inside a function or an arrow closure — must get
//! the same guarded packed-numeric access path as the identical array passed as
//! a parameter.
//!
//! Before the fix the declared type never reached the capture's read sites, so
//! `rows[i]` in a closure lowered to the fully generic `js_dyn_index_get` (27×
//! slower than the parameter form, and no faster than an untyped array — the
//! `number[]` annotation bought exactly nothing once the value was captured).
//! This matters far beyond the microbenchmark: a bundle is overwhelmingly
//! module-scope `const`s captured by closures, so in practice almost nothing was
//! reaching the fast path.
//!
//! Two things are pinned here:
//!   * `specialized_path` — the IR evidence: the captured form emits the
//!     packed-numeric loop guard and *no* `js_dyn_index_get` call at all.
//!   * `semantics_match_spec` — the behaviour, across every shape the guard's
//!     fallback has to catch: heterogeneous elements, holes, out-of-bounds,
//!     negative / fractional / non-canonical keys, a rebound array, a shrunk
//!     array, and stores. Every expectation below is the output Node prints for
//!     the same program.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Captured-array program shared by both tests. `salt` is woven into a comment
/// so each run has a distinct content hash and cannot be served from perry's
/// object cache (a cache hit re-links without re-emitting IR).
fn source(salt: &str) -> String {
    format!(
        r#"
// cache-salt: {salt}
const rows: number[] = [];
for (let i = 0; i < 8; i++) rows.push(i * 2);
// The whole point of #6369: `rows` is CAPTURED here, not passed in.
const hot = (): number => {{
  let s = 0;
  for (let r = 0; r < 3; r++) for (let i = 0; i < 8; i++) s += rows[i];
  return s;
}};
console.log("A=" + hot());

// A captured array that is NOT numeric must still read exactly like Node.
const mixed: any[] = [1, "x", null];
const readMixed = (): string =>
  String(mixed[0]) + "," + String(mixed[1]) + "," + String(mixed[2]) + "," + String(mixed[3]);
console.log("B=" + readMixed());

// Holes: a deleted element reads `undefined`, and so does everything past the end.
const holey: number[] = [1, 2, 3, 4];
delete (holey as any)[2];
const readHoley = (): string => {{
  let s = "";
  for (let i = 0; i < 6; i++) s += String(holey[i]) + ";";
  return s;
}};
console.log("C=" + readHoley());

// Negative / fractional / non-canonical / out-of-bounds keys are ordinary
// [[Get]]s, never elements — only the canonical "1" hits element 1.
const small: number[] = [10, 20, 30];
const exotic = (): string =>
  String(small[-1]) + "," + String((small as any)[1.5]) + "," + String((small as any)["1"]) +
  "," + String((small as any)["01"]) + "," + String(small[99]);
console.log("D=" + exotic());

// The capture is a *binding*: rebinding it to a different array must be seen.
let rebind: number[] = [1, 2, 3];
const sumRebind = (): number => {{
  let s = 0;
  for (let i = 0; i < 3; i++) s += rebind[i];
  return s;
}};
const d1 = sumRebind();
rebind = [10, 20, 30];
const d2 = sumRebind();
console.log("E=" + d1 + "," + d2);

// Shrinking the array under the guard: the reads past the new length are
// `undefined`, so the numeric accumulator becomes NaN (as in Node).
const shrink: number[] = [1, 2, 3, 4];
const sumShrink = (): number => {{
  let s = 0;
  for (let i = 0; i < 4; i++) s += shrink[i];
  return s;
}};
const e1 = sumShrink();
shrink.length = 2;
const e2 = sumShrink();
console.log("F=" + e1 + "," + e2);

// Element STORES through a captured array take the guarded store path.
const store: number[] = [0, 0, 0, 0];
const doStore = (): void => {{
  for (let i = 0; i < 4; i++) store[i] = i * 3;
}};
doStore();
console.log("G=" + store.join(","));
"#
    )
}

fn compile(salt: &str, keep_ir: bool) -> (tempfile::TempDir, PathBuf, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source(salt)).expect("write entry");

    let mut cmd = Command::new(perry_bin());
    cmd.current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output);
    if keep_ir {
        cmd.env("PERRY_LLVM_KEEP_IR", "1");
    }
    let compile = cmd.output().expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let stderr = String::from_utf8_lossy(&compile.stderr).into_owned();
    (dir, output, stderr)
}

/// The IR evidence from the issue: the captured read must reach the specialized
/// guarded path, and the generic dispatcher must be gone entirely.
#[test]
fn captured_numeric_array_takes_the_specialized_path() {
    let (_dir, _bin, stderr) = compile("ir", true);

    let ll_path = stderr
        .lines()
        .find_map(|line| line.split("kept LLVM IR: ").nth(1))
        .map(str::trim)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            panic!("PERRY_LLVM_KEEP_IR=1 did not report an IR path\nstderr:\n{stderr}")
        });
    let ir = std::fs::read_to_string(&ll_path).expect("read kept LLVM IR");
    let _ = std::fs::remove_file(&ll_path);

    // Counting CALL sites, not declares: a bare `declare` of an unused helper
    // proves nothing either way.
    let calls_to = |name: &str| {
        ir.lines()
            .filter(|line| line.contains(" call ") && line.contains(&format!("@{name}(")))
            .count()
    };

    assert_eq!(
        calls_to("js_dyn_index_get"),
        0,
        "a captured `number[]` must not fall back to the fully generic index \
         dispatcher — that is the 27x regression #6369 is about"
    );
    assert!(
        calls_to("js_typed_feedback_packed_f64_range_loop_guard") > 0,
        "the captured hot loop must reach the guarded packed-f64 path that the \
         identical array-as-a-parameter already gets"
    );
}

/// Behaviour, on every shape the guard's generic fallback has to catch. Each
/// expectation is what Node prints for the same program.
#[test]
fn captured_array_semantics_match_spec() {
    let (_dir, bin, _stderr) = compile("run", false);

    let run = Command::new(&bin).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "A=168\n\
         B=1,x,null,undefined\n\
         C=1;2;undefined;4;undefined;undefined;\n\
         D=undefined,undefined,20,undefined,undefined\n\
         E=6,60\n\
         F=10,NaN\n\
         G=0,3,6,9\n",
        "a captured array must keep exact JS semantics on mixed elements, holes, \
         OOB / negative / fractional / non-canonical keys, rebinding, shrinking, \
         and stores — the specialized path is only ever a guarded fast path"
    );
}
