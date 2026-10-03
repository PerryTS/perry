//! Step 5C: a completed shape's ConstFn lane names the method body, so the
//! method site calls it without loading and checking the slot's function.
//!
//! * A receiver whose class the compiler can guess (a literal or factory
//!   local, `this` in a literal method) gets a STATIC lane: the site compares
//!   the receiver word with the completed ShapeId and calls the body
//!   directly (`call @perry_closure_...`), before its learned memo.
//! * A receiver only known at run time takes the learned memo; an inherited
//!   method on a prototype whose shape carries the lane is a ConstFn entry too.
//!   A learned ConstFn hit whose body is one of the site's compile-time
//!   candidates calls that body directly.
//! * The method body's own `this.x` guard compares the completed id first.
//!
//! Every program also changes the method (another body, the same body with
//! other captures, an accessor, a delete, a prototype write) mid-loop and must
//! print node's output, ON, OFF (`PERRY_CONSTFN_SHAPE=0`) and under a forced
//! moving collector. The node outputs are pinned below.

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn clean(cmd: &mut Command) -> &mut Command {
    for (key, _) in std::env::vars() {
        if key.starts_with("PERRY_") || key == "NODE_OPTIONS" {
            cmd.env_remove(key);
        }
    }
    cmd
}

/// Compile `source` (with `PERRY_CONSTFN_SHAPE` as given; `None` = unset) and
/// return the executable and the saved LLVM IR of the entry module.
fn compile(dir: &Path, source: &str, constfn: Option<&str>) -> (PathBuf, String) {
    let src = dir.join("main.ts");
    std::fs::write(&src, source).unwrap();
    let exe = dir.join("main_bin");
    let ll_dir = dir.join("ll");
    std::fs::create_dir_all(&ll_dir).unwrap();
    let mut cmd = Command::new(perry_bin());
    clean(&mut cmd)
        .current_dir(dir)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_SAVE_LL", &ll_dir);
    if let Some(value) = constfn {
        cmd.env("PERRY_CONSTFN_SHAPE", value);
    }
    let out = cmd
        .arg("compile")
        .arg(&src)
        .arg("--no-auto-optimize")
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("compile");
    assert!(
        out.status.success(),
        "compile failed ({constfn:?})\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ll = std::fs::read_to_string(ll_dir.join("main_ts.ll")).expect("saved main_ts.ll");
    (exe, ll)
}

/// Run `exe` with the method-site report on; return (stdout, stderr).
fn run(exe: &Path, envs: &[(&str, &str)]) -> (String, String) {
    let mut cmd = Command::new(exe);
    clean(&mut cmd).env("PERRY_METHOD_SITE_STATS", "1");
    for &(k, v) in envs {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("run");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        out.status.success(),
        "run failed: {:?}\n{stderr}",
        out.status
    );
    (
        String::from_utf8_lossy(&out.stdout).trim().to_owned(),
        stderr,
    )
}

fn stat(stderr: &str, name: &str) -> u64 {
    stderr
        .split_whitespace()
        .find_map(|w| w.strip_prefix(name)?.strip_prefix('=')?.parse().ok())
        .unwrap_or(0)
}

/// The IR of the function whose `define` line contains `name`.
fn function_ir<'a>(ll: &'a str, name: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in ll.lines() {
        if line.starts_with("define ") {
            inside = line.contains(name);
        }
        if inside {
            out.push(line);
            if line == "}" {
                inside = false;
            }
        }
    }
    out
}

/// Every static-lane hit block of the module: its call line.
fn static_hit_calls(ll: &str) -> Vec<String> {
    let mut calls = Vec::new();
    let mut in_hit = false;
    for line in ll.lines() {
        if line.starts_with("msite.static_hit") {
            in_hit = true;
            continue;
        }
        if in_hit && line.contains(" = call ") {
            calls.push(line.trim().to_owned());
            in_hit = false;
        }
        if line.is_empty() {
            in_hit = false;
        }
    }
    calls
}

const MUTATIONS: &str = r#"
function mk(b: number): any { return { a: b, m: (x: number) => b * 10 + x }; }
function lit(n: number): number {
  const o: any = { a: 1, m(x: number) { return this.a + x; } };
  let h = 0;
  for (let k = 0; k < n; k++) {
    if (k === 50) o.m = function (x: number) { return 1000 + x; };
    h += o.m(k);
  }
  return h;
}
function fac(n: number): number {
  const o: any = mk(2);
  const other: any = mk(7);
  let h = 0;
  for (let k = 0; k < n; k++) {
    if (k === 30) o.m = other.m;
    if (k === 60) Object.defineProperty(o, "m", { get() { return (x: number) => 5000 + x; } });
    h += o.m(k);
  }
  return h;
}
function shorthand(n: number): number {
  const o: any = { a: 3, step(x: number) { return x + 1; } };
  let h = 0;
  for (let k = 0; k < n; k++) {
    if (k === 70) delete o.step;
    h += k < 70 ? o.step(k) : (o.step === undefined ? 1 : 0);
  }
  return h;
}
const rec: any = {
  a: 4,
  m(x: number) { return this.a * x; },
  run(n: number): number {
    let h = 0;
    for (let k = 0; k < n; k++) {
      if (k === 40) this.m = (x: number) => -x;
      h += this.m(k);
    }
    return h;
  },
};
const PROTO: any = { pa: 9, m(x: number) { return this.a + this.pa + x; } };
function mkc(a: number): any { const t: any = Object.create(PROTO); t.a = a; return t; }
function inherited(n: number): number {
  const o: any = mkc(1);
  const p: any = mkc(2);
  let h = 0;
  for (let k = 0; k < n; k++) {
    if (k === 25) p.m = (x: number) => 100 * x;
    if (k === 50) PROTO.m = function (this: any, x: number) { return this.a * 1000 + x; };
    h += o.m(k) + p.m(k);
  }
  return h;
}
console.log(lit(100), fac(100), shorthand(100), rec.run(100), inherited(100));
"#;
/// node v22 on the program above.
const MUTATIONS_NODE: &str = "55000 207650 2515 -1050 521025";

/// Static lanes call the body directly; reassignment (another body), a
/// same-body store with other captures, an accessor, a delete and prototype
/// writes all reach the generic path with node's result, ON and OFF, and
/// under a forced moving collector.
///
/// Sabotage (2026-10-03): with the store check's ConstFn deprecation skipped
/// (`deprecate_special_to_any` returning without re-stamping), `lit` keeps
/// calling the old body after `o.m = ...` and this test fails on the output.
#[test]
fn completed_shapes_call_their_body_and_every_mutation_is_seen() {
    let dir = tempfile::tempdir().unwrap();
    let (exe, ll) = compile(dir.path(), MUTATIONS, None);
    let calls = static_hit_calls(&ll);
    assert!(
        calls.len() >= 3,
        "lit, factory and `this` sites must each get a static lane: {calls:?}"
    );
    for call in &calls {
        assert!(
            call.contains("call double @perry_closure_main_ts__")
                || call.contains("call double @__perry_wrap_perry_fn_main_ts__"),
            "a static lane must be a DIRECT call of the named body: {call}"
        );
    }
    let (out, stderr) = run(&exe, &[]);
    assert_eq!(out, MUTATIONS_NODE, "{stderr}");
    assert!(
        stat(&stderr, "primes_constfn") >= 2,
        "the learned memo must hold ConstFn entries (own and inherited): {stderr}"
    );
    let (moved, stderr) = run(
        &exe,
        &[
            ("PERRY_GC_FORCE_EVACUATE", "1"),
            ("PERRY_GC_VERIFY_EVACUATION", "1"),
            ("PERRY_GC_POISON_FROMSPACE", "1"),
        ],
    );
    assert_eq!(moved, MUTATIONS_NODE, "{stderr}");

    let off_dir = tempfile::tempdir().unwrap();
    let (off_exe, off_ll) = compile(off_dir.path(), MUTATIONS, Some("0"));
    assert!(
        static_hit_calls(&off_ll).is_empty(),
        "PERRY_CONSTFN_SHAPE=0 must not emit static lanes"
    );
    let (off, stderr) = run(&off_exe, &[]);
    assert_eq!(off, MUTATIONS_NODE, "{stderr}");
    assert_eq!(stat(&stderr, "primes_constfn"), 0, "{stderr}");
}

const INHERITED: &str = r#"
const PROTO: any = {
  pa: 9,
  m(x: number) { return this.a + this.pa + x; },
  run(n: number): number { let h = 0; for (let k = 0; k < n; k++) h += this.m(k); return h; },
};
function mkc(a: number): any { const t: any = Object.create(PROTO); t.a = a; return t; }
function drive(o: any, n: number): number {
  let h = 0;
  for (let k = 0; k < n; k++) h += o.m(k);
  return h;
}
const o: any = mkc(1);
const before = drive(o, 100);
const viaThis = o.run(100);
PROTO.m = function (this: any, x: number) { return -x; };
console.log(before, viaThis, drive(o, 100), o.run(100));
"#;

/// An inherited method on a prototype whose shape carries the ConstFn lane is
/// a ConstFn entry (the prototype keeps its lanes when it is marked as one),
/// and a write to the prototype's method is seen by the next call. `this.m()`
/// in the prototype's own method has the prototype's body as a compile-time
/// candidate, so its learned ConstFn hit calls that body directly.
#[test]
fn an_inherited_constfn_method_is_a_constfn_entry_and_sees_a_prototype_write() {
    let dir = tempfile::tempdir().unwrap();
    let (exe, ll) = compile(dir.path(), INHERITED, None);
    let (out, stderr) = run(&exe, &[]);
    // node v22
    assert_eq!(out, "5950 5950 -4950 -4950", "{stderr}");
    assert!(stat(&stderr, "primes_inherited") >= 2, "{stderr}");
    assert!(
        stat(&stderr, "primes_constfn") >= 2,
        "the holder's ConstFn lane must make the inherited entries ConstFn: {stderr}"
    );
    assert!(stat(&stderr, "misses") < 20, "the sites must hit: {stderr}");
    let run_ir = function_ir(&ll, "@perry_closure_main_ts__");
    let direct = run_ir
        .iter()
        .position(|l| l.starts_with("msite.constfn_direct"))
        .unwrap_or_else(|| {
            panic!(
                "this.m() must dispatch its learned ConstFn hit to the candidate body:\n{}",
                run_ir.join("\n")
            )
        });
    assert!(
        run_ir[direct..]
            .iter()
            .find(|l| l.contains(" = call "))
            .is_some_and(|l| l.contains("call double @perry_closure_main_ts__")),
        "the candidate dispatch must be a direct call"
    );
}

/// The method body's receiver guard (`this.a` in a literal method) compares
/// the completed ConstFn id first and the birth id second: a finalized
/// literal hits on the first compare.
#[test]
fn the_body_guard_compares_the_completed_shape_first() {
    let dir = tempfile::tempdir().unwrap();
    let source = r#"
function run(n: number): number {
  const o: any = { a: 1, d: 0, m() { return this.a; } };
  let h = 0;
  for (let k = 0; k < n; k++) { o.d = k; h += o.m(); }
  return h;
}
console.log(run(10));
"#;
    let (exe, ll) = compile(dir.path(), source, None);
    assert_eq!(run(&exe, &[]).0, "10");
    let body = function_ir(&ll, "@perry_closure_main_ts__");
    let deref = body
        .iter()
        .position(|l| l.starts_with("class_field_inline.deref"))
        .unwrap_or_else(|| {
            panic!(
                "the body reads this.a through a class guard:\n{}",
                body.join("\n")
            )
        });
    let compares: Vec<&str> = body[deref..]
        .iter()
        .take_while(|l| !l.trim_start().starts_with("br "))
        .filter(|l| l.contains("icmp eq i64"))
        .copied()
        .collect();
    assert_eq!(compares.len(), 2, "completed id + birth id: {compares:?}");
    let first = compares[0].rsplit(", ").next().unwrap();
    assert!(
        first.parse::<u64>().is_ok(),
        "the first compare must be the completed shape's immediate word: {compares:?}"
    );
    let second = compares[1].rsplit(", ").next().unwrap();
    assert!(
        second.starts_with('%'),
        "the birth word (built from the live shape id) is compared second: {compares:?}"
    );
}

/// A worker builds its own literals: the static lane's compare and body are
/// valid in every agent, and a reassignment inside the worker is seen there.
#[test]
fn static_lanes_are_valid_in_a_worker_and_see_its_reassignment() {
    let dir = tempfile::tempdir().unwrap();
    let source = r#"import { spawn } from "perry/thread";
function drive(n: number): number {
  const o: any = { a: 2, m(x: number) { return this.a * x; } };
  let h = 0;
  for (let k = 0; k < n; k++) {
    if (k === 500) o.m = (x: number) => x + 1;
    h += o.m(k);
  }
  return h;
}
async function main(): Promise<void> {
  const before = drive(1000);
  const w = await spawn(() => drive(1000));
  const after = drive(1000);
  console.log(before, w, after);
}
main();
"#;
    let (exe, ll) = compile(dir.path(), source, None);
    assert!(
        !static_hit_calls(&ll).is_empty(),
        "drive's site must have a static lane"
    );
    for envs in [
        &[][..],
        &[
            ("PERRY_GC_FORCE_EVACUATE", "1"),
            ("PERRY_GC_VERIFY_EVACUATION", "1"),
        ][..],
    ] {
        let (out, stderr) = run(&exe, envs);
        assert_eq!(out, "624750 624750 624750", "{envs:?}\n{stderr}");
    }
}

const OBJECT_CREATE: &str = r#"
const PROTO: any = { pa: 9, m(x: number) { return this.a + this.pa + x; } };
const OTHER: any = { pa: 5, m(x: number) { return this.a * 100 + this.pa + x; } };
function mkc(a: number): any {
  const t: any = Object.create(PROTO);
  t.a = a;
  if (a === 3) Object.setPrototypeOf(t, OTHER);
  return t;
}
const g: any = mkc(4);
function local(n: number): number {
  const o: any = mkc(1);
  let h = 0;
  for (let k = 0; k < n; k++) h += o.m(k);
  return h;
}
function captured(n: number): number {
  const o: any = mkc(2);
  const go = (m: number): number => {
    let h = 0;
    for (let k = 0; k < m; k++) h += o.m(k);
    return h;
  };
  return go(n);
}
function modconst(n: number): number {
  let h = 0;
  for (let k = 0; k < n; k++) h += g.m(k);
  return h;
}
function wrong(n: number): number {
  const o: any = mkc(3);
  const p: any = mkc(5);
  let h = 0;
  for (let k = 0; k < n; k++) {
    if (k === 40) p.m = (x: number) => 7 * x;
    h += o.m(k) + p.m(k);
  }
  return h;
}
function replaced(n: number): number {
  const o: any = mkc(6);
  let h = 0;
  for (let k = 0; k < n; k++) {
    if (k === 50) PROTO.m = function (this: any, x: number) { return this.a * 1000 + x; };
    h += o.m(k);
  }
  return h;
}
console.log(local(100), captured(100), modconst(100), wrong(100), replaced(100));
"#;
/// node v22 on the program above.
const OBJECT_CREATE_NODE: &str = "5950 6050 6250 65980 305700";

/// A value made by `Object.create(PROTO)` and returned from a factory (into a
/// local, a captured local, a module const) has PROTO's literal class as its
/// candidate: the learned inherited ConstFn hit calls PROTO's `m` body
/// directly, and no static compare is emitted (the receiver inherits `m`, so
/// it never carries PROTO's own shape). The candidate is only a guess: a
/// receiver whose prototype was replaced (`wrong`'s `o`), one with an own `m`
/// (`wrong`'s `p`), and a write to PROTO.m (`replaced`) all give node's result.
///
/// Sabotage (2026-10-03): with the candidate called without comparing the
/// recorded body (`msite.constfn_direct` taken unconditionally), `wrong`
/// calls PROTO's body for an OTHER receiver and this test fails on the output.
#[test]
fn object_create_values_call_the_prototype_body_directly_and_a_wrong_candidate_misses() {
    let dir = tempfile::tempdir().unwrap();
    let (exe, ll) = compile(dir.path(), OBJECT_CREATE, None);
    for name in ["local", "modconst", "wrong", "replaced"] {
        let ir = function_ir(&ll, &format!("@perry_fn_main_ts__{name}("));
        assert!(
            !ir.iter().any(|l| l.starts_with("msite.static_hit")),
            "{name}: an Object.create receiver must not compare PROTO's own shape"
        );
        let direct = ir
            .iter()
            .position(|l| l.starts_with("msite.constfn_direct"))
            .unwrap_or_else(|| {
                panic!(
                    "{name}: the learned ConstFn hit must dispatch to PROTO's body:\n{}",
                    ir.join("\n")
                )
            });
        assert!(
            ir[direct..]
                .iter()
                .find(|l| l.contains(" = call "))
                .is_some_and(|l| l.contains("call double @perry_closure_main_ts__")),
            "{name}: the candidate dispatch must be a direct call"
        );
    }
    for envs in [
        &[][..],
        &[
            ("PERRY_GC_FORCE_EVACUATE", "1"),
            ("PERRY_GC_VERIFY_EVACUATION", "1"),
            ("PERRY_GC_POISON_FROMSPACE", "1"),
        ][..],
    ] {
        let (out, stderr) = run(&exe, envs);
        assert_eq!(out, OBJECT_CREATE_NODE, "{envs:?}\n{stderr}");
        assert!(
            stat(&stderr, "primes_constfn") >= 4,
            "the inherited entries must be ConstFn entries: {stderr}"
        );
    }

    let off_dir = tempfile::tempdir().unwrap();
    let (off_exe, off_ll) = compile(off_dir.path(), OBJECT_CREATE, Some("0"));
    assert!(
        !off_ll
            .lines()
            .any(|l| l.starts_with("msite.constfn_direct")),
        "PERRY_CONSTFN_SHAPE=0 has no candidate bodies"
    );
    let (off, stderr) = run(&off_exe, &[]);
    assert_eq!(off, OBJECT_CREATE_NODE, "{stderr}");
}
