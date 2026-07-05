//! Regression tests for the 2026-07-02 audit's capture-re-registration P0:
//! the end-of-body `RegisterClassCaptures` refresh in arrow/function-decl
//! bodies (a) ran AFTER `ctx.class_renames` was restored and looked the
//! class up by its RAW ident, and (b) inserted only before a TRAILING
//! return.
//!
//! (a) meant factory B's `class e` (renamed `e$0` when factory A's `class e`
//! was lowered first) re-registered factory A's `e` with B's out-of-scope
//! ids — codegen's LocalGet soft-fallback then clobbered A's snapshot to
//! all-undefined the moment B ran (minified bundles reuse one-letter class
//! names across hundreds of factories, so both preconditions were
//! near-certain at bundle scale); `e$0` itself never got refreshed. (b)
//! meant early-return paths kept the stale decl-site snapshot.
//!
//! Static-method reads go through the snapshot-only prologue, so they
//! discriminate the snapshot's value directly. Expected outputs are
//! byte-for-byte `node --experimental-strip-types`.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed (exit {:?})\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// Two arrow factories each declaring `class e` with their own captures:
/// running factory B must not clobber factory A's capture snapshot (A's
/// static read still sees "alpha" after B ran).
#[test]
fn same_class_name_across_factories_keeps_snapshots_separate() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const makeA = () => {
  const tagA = "alpha";
  class e {
    static who() {
      return tagA;
    }
  }
  return () => e.who();
};
const whoA = makeA();
const makeB = (x: number) => {
  const tagB = "beta" + x;
  class e {
    static who() {
      return tagB;
    }
  }
  return e.who();
};
console.log("A1:", whoA());
console.log("B:", makeB(1));
console.log("A2:", whoA());
"#,
    );
    assert_eq!(stdout, "A1: alpha\nB: beta1\nA2: alpha\n");
}

/// The snapshot refresh must run before EVERY return, not only a trailing
/// one: an early return after a captured-var mutation must see the mutated
/// value in the snapshot (static reads are snapshot-only).
#[test]
fn snapshot_refresh_covers_early_returns() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function makeC(flag: boolean) {
  let n = 1;
  class e {
    static val() {
      return n;
    }
  }
  n = 2;
  if (flag) return e.val;
  n = 3;
  return e.val;
}
console.log("C-early:", makeC(true)());
console.log("C-late:", makeC(false)());
"#,
    );
    assert_eq!(stdout, "C-early: 2\nC-late: 3\n");
}

/// Audit P0-B: a same-body assignment AFTER the class declaration must be
/// visible to a mid-body construct (the decl-site snapshot is authoritative
/// at construct time, so it must track assignments), and the capture
/// write-back must not reset the outer local to the stale value.
#[test]
fn mid_body_construct_sees_assignment_after_class_decl() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function f() {
  let x = 1;
  class C {
    m() {
      return x;
    }
  }
  x = 2;
  const c = new C();
  console.log("m:", c.m(), "x:", x);
}
f();
"#,
    );
    assert_eq!(stdout, "m: 2 x: 2\n");
}

/// Assignment inside an if-branch after the declaration (the zod
/// enum-namespace-after-class shape): static reads are snapshot-only, so
/// the branch's refresh must land at its own nesting level.
#[test]
fn branch_assignment_refreshes_snapshot() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function g(flag: boolean) {
  let cfg: any = null;
  class D {
    static read() {
      return cfg === null ? "null" : cfg.mode;
    }
  }
  if (flag) {
    cfg = { mode: "live" };
  }
  console.log("D:", D.read());
}
g(true);
g(false);
"#,
    );
    assert_eq!(stdout, "D: live\nD: null\n");
}

/// Statics on a RENAMED colliding class must dispatch to the renamed
/// registrant, not the first same-named one: static method calls, static
/// FIELD reads/writes, and instance construction in factory B must all
/// bind B's `class e` (#5938 follow-up — the raw-name lookups in the
/// static-call/static-field lowering arms bound factory A's class, so
/// `B:` printed alpha values).
#[test]
fn renamed_class_statics_dispatch_to_renamed_registrant() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const mkA = () => {
  const tag = "alpha";
  class e {
    static label = "L:" + tag;
    v: string;
    constructor() {
      this.v = tag;
    }
    static who(): string {
      return tag;
    }
  }
  e.label = e.label + "!";
  return { who: () => e.who(), inst: () => new e().v, lab: () => e.label };
};
const a = mkA();
const mkB = (x: number) => {
  const tag = "beta" + x;
  class e {
    static label = "L:" + tag;
    v: string;
    constructor() {
      this.v = tag;
    }
    static who(): string {
      return tag;
    }
  }
  e.label = e.label + "?";
  return { who: e.who(), inst: new e().v, lab: e.label };
};
console.log(a.who(), a.inst(), a.lab());
const b = mkB(1);
console.log(b.who, b.inst, b.lab);
console.log(a.who(), a.inst(), a.lab());
"#,
    );
    assert_eq!(
        stdout,
        "alpha alpha L:alpha!\nbeta1 beta1 L:beta1?\nalpha alpha L:alpha!\n"
    );
}
