//! Immutable layouts carry ConstFn method bodies. Receiver mutation,
//! inherited replacement, workers, and Object.create keep Node's results.
//! The layout entries are checked with ConstFn enabled and disabled; runtime
//! cache-entry counters and the removed learned-dispatch blocks are retired.

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
        .env("PERRY_SAVE_LL", &ll_dir)
        .env("PERRY_GC_INSTRUMENTS", "1");
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
    clean(&mut cmd);
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

/// The IR of the function whose `define` line contains `name`.

/// Every static-lane hit block of the module: its call line.

fn constfn_layouts(ll: &str) -> usize {
    ll.lines()
        .filter(|line| {
            line.starts_with("@perry_constfn_final_") && line.contains(" = private constant [")
        })
        .count()
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
    assert!(
        constfn_layouts(&ll) > 0,
        "ConstFn layout entries must be emitted"
    );
    for envs in [
        &[][..],
        &[
            ("PERRY_GC_FORCE_EVACUATE", "1"),
            ("PERRY_GC_VERIFY_EVACUATION", "1"),
        ][..],
    ] {
        let (out, stderr) = run(&exe, envs);
        assert_eq!(out, MUTATIONS_NODE, "{envs:?}\n{stderr}");
    }
    let off_dir = tempfile::tempdir().unwrap();
    let (off_exe, off_ll) = compile(off_dir.path(), MUTATIONS, Some("0"));
    assert_eq!(
        constfn_layouts(&off_ll),
        0,
        "disabled ConstFn layout entries"
    );
    let (off, stderr) = run(&off_exe, &[]);
    assert_eq!(off, MUTATIONS_NODE, "{stderr}");
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
fn inherited_constfn_methods_observe_prototype_writes() {
    let dir = tempfile::tempdir().unwrap();
    let (exe, ll) = compile(dir.path(), INHERITED, None);
    assert!(constfn_layouts(&ll) > 0, "prototype ConstFn layout entries");
    let (out, stderr) = run(&exe, &[]);
    assert_eq!(out, "5950 5950 -4950 -4950", "{stderr}");
}

/// The method body's `this.a` (a literal method whose receiver completes on a
/// ConstFn shape) is a shape-site read (#12483): the site's word follows the
/// shape the receiver actually has, completed or not, so no class guard
/// against the literal's birth or completed id is emitted for it.
#[test]
fn literal_methods_read_the_current_receiver() {
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
    assert!(constfn_layouts(&ll) > 0, "literal method layout");
    assert_eq!(run(&exe, &[]).0, "10");
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
    assert!(constfn_layouts(&ll) > 0, "worker literal method layout");
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
fn object_create_methods_follow_live_prototype_bodies() {
    let dir = tempfile::tempdir().unwrap();
    let (exe, ll) = compile(dir.path(), OBJECT_CREATE, None);
    assert!(constfn_layouts(&ll) > 0, "prototype ConstFn layout entries");
    for envs in [
        &[][..],
        &[
            ("PERRY_GC_FORCE_EVACUATE", "1"),
            ("PERRY_GC_VERIFY_EVACUATION", "1"),
        ][..],
    ] {
        let (out, stderr) = run(&exe, envs);
        assert_eq!(out, OBJECT_CREATE_NODE, "{envs:?}\n{stderr}");
    }
    let off_dir = tempfile::tempdir().unwrap();
    let (off_exe, off_ll) = compile(off_dir.path(), OBJECT_CREATE, Some("0"));
    assert_eq!(constfn_layouts(&off_ll), 0);
    let (off, stderr) = run(&off_exe, &[]);
    assert_eq!(off, OBJECT_CREATE_NODE, "{stderr}");
}
