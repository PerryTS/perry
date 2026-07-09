//! Regression tests: Object enumeration + expando properties on Map/Set
//! receivers.
//!
//! A `Map`/`Set` is a `MapHeader`/`SetHeader` cell, NOT an `ObjectHeader`.
//! Pre-fix:
//!
//! - `Object.keys` / `Object.values` / `Object.entries` / `for…in` on a Map
//!   or Set read collection-internal bytes as a `keys_array` pointer and
//!   SIGSEGV'd in `js_array_length`'s GC-kind probe (a deterministic crash:
//!   `Object.keys(new Map())` alone reproduced it). A telemetry path in a
//!   large esbuild-bundled CLI app hit this via `Object.keys(cache)` on a
//!   lodash-memoize Map cache and took the whole process down mid-turn.
//! - a plain expando write (`m.foo = 42`) bit-cast the MapHeader as an
//!   ObjectHeader and overwrote collection-internal fields (silent memory
//!   corruption), then crashed on the next enumeration.
//!
//! Fix: `ExoticKind::Map`/`Set` route expando get/set/has/delete through the
//! exotic side table (the same mechanism as Date/RegExp/Error/Promise), and
//! the keys/values/entries entry points enumerate exactly the side-table
//! expandos (collection DATA lives in internal slots — Node returns `[]`
//! for `Object.keys(new Map([...]))`).

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
        "compiled binary failed (pre-fix: SIGSEGV in js_array_length's \
         GC-kind probe)\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// The bare crash cases: every Object enumeration entry point over Map/Set
/// receivers (pre-fix: SIGSEGV on all four).
#[test]
fn object_enumeration_over_map_set_does_not_crash() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const m: any = new Map([["a", 1], ["b", 2]]);
console.log(JSON.stringify(Object.keys(m)));
console.log(JSON.stringify(Object.values(m)));
console.log(JSON.stringify(Object.entries(m)));
const ks: string[] = []; for (const k in m) ks.push(k);
console.log(JSON.stringify(ks));
const s: any = new Set([1, 2]);
console.log(JSON.stringify(Object.keys(s)));
console.log(JSON.stringify(Object.keys(new Map())));
"#,
    );
    assert_eq!(stdout, "[]\n[]\n[]\n[]\n[]\n[]\n");
}

/// Expando writes must land in the side table (not corrupt the collection),
/// read back, enumerate, and leave the collection's own data intact.
#[test]
fn map_set_expando_properties_round_trip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const m: any = new Map([["a", 1]]);
m.foo = 42;
console.log("get:", m.foo);
console.log("keys:", JSON.stringify(Object.keys(m)));
console.log("hasOwn:", m.hasOwnProperty("foo"), Object.hasOwn(m, "foo"));
console.log("data intact:", m.get("a"), m.size);
const ks: string[] = []; for (const k in m) ks.push(k);
console.log("forin:", JSON.stringify(ks));
const s: any = new Set([7]);
s.tag = "x";
console.log("set expando:", s.tag, s.size, s.has(7), JSON.stringify(Object.keys(s)));
"#,
    );
    assert_eq!(
        stdout,
        "get: 42\nkeys: [\"foo\"]\nhasOwn: true true\ndata intact: 1 1\n\
         forin: [\"foo\"]\nset expando: x 1 true [\"tag\"]\n"
    );
}

/// The exact shape that took down the CLI app: a lodash-memoize-style cache
/// (`fn.cache = new Map()`) enumerated by a telemetry helper.
#[test]
fn memoize_cache_map_object_keys() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function memoize(fn: any) {
  const memoized: any = (...args: any[]) => {
    const key = args[0];
    if (memoized.cache.has(key)) return memoized.cache.get(key);
    const v = fn(...args);
    memoized.cache.set(key, v);
    return v;
  };
  memoized.cache = new Map();
  return memoized;
}
const f = memoize((x: number) => x * 2);
f(1); f(2); f(1);
const K: any = f.cache;
console.log(typeof K === "object" ? JSON.stringify(Object.keys(K)) : "-");
console.log("cache works:", f.cache.get(1), f.cache.size);
"#,
    );
    assert_eq!(stdout, "[]\ncache works: 2 2\n");
}
