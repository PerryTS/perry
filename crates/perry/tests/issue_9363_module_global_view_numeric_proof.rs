//! #9363 (A): a module-global typed-array receiver must carry the same
//! Number-by-construction proof a body-local `const` view does.
//!
//! `collectors/ptr_shape_numeric.rs` proved `view[i]` is Number-or-`undefined`
//! only from `numeric_ta_views` (spec-proven `TaPtr` params) or
//! `const_local_inits` (a compiler-visible `const` init in the SCANNED body).
//! A module-global `const buf = new Uint8Array(N)` read inside a function had
//! neither, so `acc += buf[i]` lost the accumulator's numeric proof, and the
//! add lowered through the rooted `guarded_add` diamond — a GC shadow-frame
//! load + store + `js_write_barrier_root_nanbox` per element, plus the
//! dynamic-add cold arm.
//!
//! Measured: 444 ms -> 94 ms, identical to the body-local receiver, against
//! node's 79.
//!
//! ATTRIBUTION, corrected by measurement. The missing proof ALSO leaves a
//! per-iteration `load volatile @PERRY_GC_POLL_ARMED` in the loop
//! (`loop_may_allocate` stays conservative when the `+` is not inert), and the
//! obvious story is that the volatile load blocks vectorization. It does not
//! pay: removing the poll under the same construction proof measured 94 ms
//! either way, and did not vectorize either. The whole 4.7x is the rooting
//! diamond. See the note in the test body and #9363.
//!
//! The pin is structural rather than a timing assertion: the dynamic-add
//! helper is the artifact the missing proof produced, and it is absent iff the
//! proof is present.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// `viaGlobal` reads a module-global view; `viaLocalConst` reads a body-local
/// one. The two must compile to the same shape of inner loop — that equality
/// is the fact under test, and it is what makes `viaLocalConst` a control
/// rather than a second assertion.
const SOURCE: &str = r#"
const N = 4096;
const gbuf = new Uint8Array(N);
for (let i = 0; i < N; i++) gbuf[i] = i % 256;

function viaGlobal(): number {
    let s = 0;
    for (let i = 0; i < N; i++) s += gbuf[i];
    return s;
}

function viaLocalConst(): number {
    const b = new Uint8Array(N);
    for (let i = 0; i < N; i++) b[i] = i % 256;
    let s = 0;
    for (let i = 0; i < N; i++) s += b[i];
    return s;
}

console.log(viaGlobal() + "," + viaLocalConst());
"#;

const EXPECTED: &str = "522240,522240\n";

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

fn kept_ir(stderr: &str) -> String {
    let path = stderr
        .lines()
        .find_map(|line| line.split("kept LLVM IR: ").nth(1))
        .map(str::trim)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("PERRY_LLVM_KEEP_IR did not report an IR path\n{stderr}"));
    std::fs::read_to_string(path).expect("read kept LLVM IR")
}

/// The body of one emitted function, by its perry symbol suffix.
fn function_body(ir: &str, suffix: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for line in ir.lines() {
        if line.starts_with("define ") {
            inside = line.contains(&format!("__{suffix}("));
        } else if inside {
            if line.starts_with('}') {
                break;
            }
            out.push_str(line);
            out.push('\n');
        }
    }
    assert!(!out.is_empty(), "no emitted body found for `{suffix}`");
    out
}

fn count(body: &str, needle: &str) -> usize {
    body.lines().filter(|l| l.contains(needle)).count()
}

fn run(bin: &Path, dir: &Path) -> Output {
    Command::new(bin)
        .current_dir(dir)
        .output()
        .expect("run compiled binary")
}

/// A module-global view receiver earns the same numeric proof as a body-local
/// one: a native `fadd` rather than the rooted dynamic-add diamond.
#[test]
fn module_global_view_receiver_earns_the_numeric_proof() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, stderr) = compile(dir.path(), SOURCE, &[]);
    let ir = kept_ir(&stderr);

    let global = function_body(&ir, "viaGlobal");
    let local = function_body(&ir, "viaLocalConst");

    // The control must itself be clean, or the equality below proves nothing.
    assert_eq!(
        count(&local, "js_dynamic_string_or_number_add"),
        0,
        "control (body-local receiver) unexpectedly lost its numeric proof — \
         this test can no longer distinguish anything"
    );

    assert_eq!(
        count(&global, "js_dynamic_string_or_number_add"),
        0,
        "module-global receiver still routes `acc += buf[i]` through the \
         dynamic-add helper: the accumulator lost its Number-by-construction \
         proof"
    );

    // NOT asserted: that the module-global body has no GC poll. It still has
    // one, because `can_lower_buffer_access_without_calls` demands a tracked
    // `buffer_view_slots` entry that a module global never gets, so
    // `loop_may_allocate` stays conservative. Admitting the read as inert
    // under the construction proof was implemented and MEASURED FLAT (94 ms
    // either way), and it did not unblock vectorization either — the
    // remaining blocker is #9360's per-element admission-cache probe, a
    // control-flow diamond in the loop body. Since `expr_is_inert_primitive`
    // also governs rooting decisions, an unmeasured widening of it does not
    // ship. See #9363.

    let out = run(&bin, dir.path());
    assert!(out.status.success(), "binary failed");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        EXPECTED,
        "both receivers must sum identically"
    );
}

/// The proof is a CONSTRUCTION proof, not an annotation: a reassigned
/// module-global must not be admitted, because a later write can put anything
/// in the binding. `module_global_proven_types` already excludes reassigned
/// bindings — this pins that the exclusion is load-bearing here.
#[test]
fn reassigned_module_global_is_not_admitted() {
    const REASSIGNED: &str = r#"
const N = 64;
let gbuf: any = new Uint8Array(N);
for (let i = 0; i < N; i++) gbuf[i] = i;

function sum(): number {
    let s = 0;
    for (let i = 0; i < N; i++) s += gbuf[i];
    return s;
}

const first = sum();
gbuf = "not a buffer";
console.log(first + "," + typeof gbuf);
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, _stderr) = compile(dir.path(), REASSIGNED, &[]);
    let out = run(&bin, dir.path());
    assert!(out.status.success(), "binary failed");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "2016,string\n",
        "a reassigned module global must still read correctly"
    );
}
