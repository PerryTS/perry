//! A typed array's length may be an `!invariant.load`, and its elements read
//! from inline storage against the construction length, only while nothing
//! can observe its `.buffer`. Observing `.buffer` rebinds a fresh
//! `new TA(n)` to an external backing, and `buffer.transfer()` then detaches
//! it: the length reads 0 and every element read is `undefined`. A call that
//! reaches the array can do both, so the length must be reloaded after it.
//!
//! The proof is `collectors/sealed_buffers.rs`: a binding is sealed when every
//! use is an element access with a never-string key, a `.length` read, or an
//! argument of a module function whose parameter is itself sealed.
//!
//! - The checked tier of a computed-length local passed to a function that
//!   reads `.buffer` (and so can detach it, inside the loop) carries no
//!   invariant length load and no checked access trusting the construction:
//!   the loop contains the exposing call, so the whole loop, back edge
//!   included, runs untrusted (the bound is not a literal, so the loop is not
//!   unrolled into straight-line code whose first read precedes the call);
//!   its sealed twin, passed to a function that only indexes it, keeps a
//!   native view with the invariant length load (the witness that the
//!   assertion is not vacuous).
//! - A literal-length module array whose `.buffer` is observed takes no
//!   `$spec_ta*` clone; its sealed twin does.
//!
//! The behavioural half is `test-files/test_gap_typed_array_detach_*.ts`,
//! compared against node.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const CHECKED: &str = r#"
function size(): number { return 8; }
function peek(t: Float64Array, i: number): number { return t[i]; }
function kill(t: Float64Array): void { (t.buffer as any).transfer(); }
function exposed(k: number, j: number): number {
  const n = size();
  const a = new Float64Array(n * 1);
  let s = 0;
  for (let i = 0; i < n; i++) {
    const v = a[(i + j) & 7];
    s += v === undefined ? 1000 : v;
    if (i === k) kill(a);
  }
  return s;
}
function sealed(k: number, j: number): number {
  const n = size();
  const a = new Float64Array(n * 1);
  let s = 0;
  for (let i = 0; i < n; i++) {
    const v = a[(i + j) & 7];
    s += v === undefined ? 1000 : v;
    if (i === k) s += peek(a, 0);
  }
  return s;
}
console.log(exposed(3, 1), sealed(3, 1));
"#;

const SPEC: &str = r#"
function sum7(a: Float64Array): number {
  let s = 0;
  for (let i = 0; i < 5; i++) { const o = i * 7; s += a[o] + a[o + 6]; }
  return s;
}
function sum8(a: Float64Array): number {
  let s = 0;
  for (let i = 0; i < 8; i++) s += a[i];
  return s;
}
const sealedArray = new Float64Array(35);
const exposedArray = new Float64Array(8);
console.log(sum7(sealedArray), sum8(exposedArray));
const moved = (exposedArray.buffer as any).transfer();
console.log(sum8(exposedArray), moved.byteLength);
"#;

const BUFFER_PARAM: &str = r#"
function det(b: Buffer): void { (b.buffer as any).transfer(); }
function bufparam(t: Buffer, k: number): number {
  let s = 0;
  for (let i = 0; i < t.length; i++) {
    const v = t[i];
    s += v === undefined ? 1000 : v;
    if (i === k) det(t);
  }
  return s;
}
const bp = Buffer.alloc(12);
for (let i = 0; i < 12; i++) bp[i] = i;
console.log(bufparam(bp, 3));
"#;

/// The body of the first `define` whose symbol starts with `prefix`.
fn function_body<'a>(ir: &'a str, prefix: &str) -> Option<&'a str> {
    let needle = format!("@{prefix}");
    let mut offset = 0;
    for line in ir.split_inclusive('\n') {
        if line.starts_with("define ") && line.contains(&needle) {
            let rest = &ir[offset..];
            let end = rest.find("\n}\n").map(|i| i + 3).unwrap_or(rest.len());
            return Some(&rest[..end]);
        }
        offset += line.len();
    }
    None
}

fn compile_ir(name: &str, source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join(format!("{name}.ts"));
    std::fs::write(&entry, source).expect("write entry");
    let ll = dir.path().join("ll");
    std::fs::create_dir_all(&ll).expect("ll dir");
    let out = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(dir.path().join(name))
        .arg("--no-cache")
        .arg("--no-link")
        .env("PERRY_SAVE_LL", &ll)
        .output()
        .expect("run perry compile");
    assert!(
        out.status.success(),
        "compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    std::fs::read_dir(&ll)
        .expect("read ll dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "ll"))
        .map(|e| std::fs::read_to_string(e.path()).expect("read .ll"))
        .collect()
}

fn count(body: &str, needle: &str) -> usize {
    body.matches(needle).count()
}

/// Checked element reads: one `pview.get.oob` block each.
fn checked_reads(body: &str) -> usize {
    body.lines()
        .filter(|l| l.starts_with("pview.get.oob."))
        .count()
}

#[test]
fn a_detachable_array_has_no_invariant_length_across_the_call() {
    let ir = compile_ir("checked", CHECKED);
    let sealed = function_body(&ir, "perry_fn_checked_ts__sealed")
        .unwrap_or_else(|| panic!("no sealed function in:\n{ir}"));
    let exposed = function_body(&ir, "perry_fn_checked_ts__exposed")
        .unwrap_or_else(|| panic!("no exposed function in:\n{ir}"));
    // Witness: the sealed twin of the same shape is a trusted native view,
    // whose length is an invariant load and whose reads call no runtime.
    assert!(
        count(sealed, "!invariant.load") > 0 && count(sealed, "@js_typed_array_get") == 0,
        "sealed twin lost its native view:\n{sealed}"
    );
    // `kill(a)` can detach `a` between two iterations: no invariant length,
    // and no checked access against a cached length.
    assert_eq!(
        count(exposed, "!invariant.load"),
        0,
        "invariant load in a function whose call can detach the array:\n{exposed}"
    );
    assert_eq!(
        checked_reads(exposed),
        0,
        "checked tier trusts a detachable array:\n{exposed}"
    );
}

#[test]
fn an_observed_buffer_takes_no_specialized_clone() {
    let ir = compile_ir("spec", SPEC);
    assert!(
        function_body(&ir, "perry_fn_spec_ts__sum7$spec_ta").is_some(),
        "the sealed array lost its specialized clone:\n{ir}"
    );
    assert!(
        function_body(&ir, "perry_fn_spec_ts__sum8$spec_ta").is_none(),
        "an array whose .buffer is observed got a specialized clone:\n{ir}"
    );
}

#[test]
fn a_buffer_param_reloads_its_length_after_the_call() {
    // A declared `Buffer` parameter is a view over whatever the caller
    // passed, and `det(t)` detaches it mid-loop: neither the loop condition
    // `t.length` nor the access may use an invariant length or a guard proof
    // taken before the call.
    let ir = compile_ir("bufparam", BUFFER_PARAM);
    let body = function_body(&ir, "perry_fn_bufparam_ts__bufparam(")
        .unwrap_or_else(|| panic!("no bufparam function in:\n{ir}"));
    assert_eq!(
        count(body, "!invariant.load"),
        0,
        "invariant length load of a detachable Buffer param:\n{body}"
    );
}
