//! Regression test for #5437 (Next.js `f.get(r, u, context)` wall): the inline
//! method-dispatch tower (`idispatch`) used a SINGLE arg-bundling decision for
//! ALL candidate implementors of a method name. When at least one implementor
//! had a `...rest` param, every case — including non-rest implementors with
//! more positional params — received the args bundled into a single rest array
//! passed as arg0, so the other positional params read uninitialized `0.0`.
//!
//! In the Next.js bundle, `get` is implemented by `LRUCache.get` (arity 1),
//! `CacheHandler.get` (arity 2), `ResponseCache.get` (arity 3, the `(key,
//! responseGenerator, context)` async method) and several rest-bearing webpack
//! `get`s. The rest-bundle therefore collapsed `nh.get(key, gen, context)` into
//! `nh.get([key, gen, context])`, leaving `context` (the 3rd param) as `0.0`.
//! Destructuring `const { incrementalCache } = context` then read off the
//! number `0.0` → `incrementalCache` undefined → `Cannot read properties of
//! undefined (reading 'get')` at render time.
//!
//! Fix: the dispatch tower builds each case's args from the raw user args
//! applying THAT implementor's own arity/rest-ness — a non-rest callee gets its
//! positional params (padded with `undefined`), a rest callee gets the trailing
//! args bundled at its rest slot.

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
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// A method name (`get`) implemented by both a `...rest` class and non-rest
/// classes with differing positional arity, dispatched on an `any` receiver so
/// codegen emits the inline class-id switch tower. The 3-arg call must reach
/// `ThreeArg.get` with all three positional params (pre-fix the 3rd param read
/// `0.0`), the rest implementor must still receive a bundled array, and the
/// 1-arg implementor must still see only its first param.
#[test]
fn idispatch_tower_uses_per_implementor_arity() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
class WithRest {
  get(...r: any[]): string { return "rest:" + r.length; }
}
class ThreeArg {
  get(a: any, b: any, c: any): string {
    return "three:" + String(a) + "," + String(b) + "," + String(c);
  }
}
class OneArg {
  get(a: any): string { return "one:" + String(a); }
}

function pick(n: number): any {
  if (n === 0) return new WithRest();
  if (n === 1) return new ThreeArg();
  return new OneArg();
}

const r0: any = pick(0);
const r1: any = pick(1);
const r2: any = pick(2);
console.log(r0.get(10, 20, 30));
console.log(r1.get(10, 20, 30));
console.log(r2.get(10, 20, 30));
"#,
    );
    // node --experimental-strip-types prints exactly this.
    assert_eq!(stdout, "rest:3\nthree:10,20,30\none:10\n");
}
