//! Regression test for #5437 (Next.js `context.incrementalCache.get(...)` wall).
//!
//! This is the end-to-end shape that the per-implementor-arity dispatch fix
//! (#5622, `crates/perry-codegen/src/lower_call/property_get.rs`) unblocks. Two
//! walls lived on this exact chain in the Next.js app-router render:
//!
//!   1. `Cannot read properties of undefined (reading 'get')` — the 3rd
//!      positional arg `context` of `responseCache.get(key, gen, context)` was
//!      collapsed to `0.0` because `get` has a `...rest` implementor and the
//!      dispatch tower applied ONE global rest-bundle to every case, dropping
//!      the non-rest 3-arg implementor's positional params. `const {
//!      incrementalCache } = context` then destructured off `0.0`.
//!
//!   2. `get is not a function` — once `context` survives, the chained
//!      `context.incrementalCache.get(key, { kind })` must itself resolve
//!      through the (multi-implementor) dispatch tower. If the tower mis-routes
//!      or the receiver's class-id misses every case, it falls through to
//!      `js_native_call_method`'s catch-all and throws the bare
//!      `get is not a function`.
//!
//! With the per-implementor-arity fix both walls are gone: `context` arrives
//! intact AND the chained async `incrementalCache.get` dispatches to the right
//! 2-arg method. This test models the chain (object-literal 3rd arg carrying an
//! IncrementalCache instance; `get` co-implemented by a `...rest` class and a
//! 1-arg class so the tower has multiple cases) and asserts the same byte
//! output as `node --experimental-strip-types`.

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

/// The 3rd positional arg of an untyped `f.get(key, gen, context)` call is an
/// object literal carrying an `IncrementalCache` instance. `f.get` (the
/// non-rest 3-arg `ResponseCache.get`) must receive `context` intact, destructure
/// `incrementalCache` off it, and the chained async `incrementalCache.get(key,
/// { kind })` must dispatch to the 2-arg method WITHOUT "get is not a function" —
/// even though `get` is also implemented by a `...rest` class and a 1-arg class.
#[test]
fn incremental_cache_get_chain_resolves() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
class IncrementalCache {
  async get(cacheKey: string, ctx: { kind: string }): Promise<string> {
    return "IC.get(" + cacheKey + "," + ctx.kind + ")";
  }
}

// Sibling `get` implementors with differing arity, including a ...rest one, so
// the dynamic-dispatch tower has multiple cases (the shape that made the
// rest-bundle collapse every case pre-#5622).
class WithRest {
  get(...r: any[]): string { return "rest:" + r.length; }
}
class OneArg {
  get(a: any): string { return "one:" + String(a); }
}

class ResponseCache {
  // 3-arg async method; 3rd param `context` MUST survive dispatch (pre-#5622 it
  // read 0.0 → `const { incrementalCache } = context` off a number).
  async get(key: string, gen: number, context: any): Promise<string> {
    const { incrementalCache } = context;
    const r = await incrementalCache.get(key, { kind: "APP" });
    return "handleGet[" + r + "]";
  }
}

function pickGetter(n: number): any {
  if (n === 0) return new WithRest();
  if (n === 1) return new ResponseCache();
  return new OneArg();
}

async function main() {
  const f: any = pickGetter(1);
  const ic: any = new IncrementalCache();
  // 3rd positional arg = object literal carrying the IncrementalCache instance.
  const out = await f.get("k1", 7, { routeKind: "APP", incrementalCache: ic, waitUntil: null });
  console.log(out);
  // Exercise the rest + one-arg siblings so the tower keeps >1 implementor.
  console.log(pickGetter(0).get(1, 2, 3));
  console.log(pickGetter(2).get(9, 9, 9));
}
main();
"#,
    );
    // node --experimental-strip-types prints exactly this.
    assert_eq!(stdout, "handleGet[IC.get(k1,APP)]\nrest:3\none:9\n");
}
