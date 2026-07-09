//! Regression: any-typed `.keys()` / `.entries()` / `.values()` on a Web
//! `Headers` returned an EMPTY iterator, while `.has()` / `.get()` and the
//! computed form `headers["keys"]()` worked.
//!
//! Root cause: perry-hir's #597 catch-all folds an any-typed
//! `.keys()`/`.entries()`/`.values()` call to `Expr::Array{Keys,Entries,Values}`
//! (so any-typed *real* arrays iterate correctly through the index-based for-of
//! lowering). A `Headers` instance is a fetch-band registry **handle**, not a
//! heap `ArrayHeader`, so `js_array_{keys,entries,values}_iter_obj` read the
//! handle id as an array length and yielded nothing.
//!
//! Impact: a large bundled SDK builds auth headers, wraps them as
//! `{ values: Headers }`, and merges via `yield* wrapper.values.entries()`.
//! Because `wrapper.values` is any-typed, `.entries()` folded to `ArrayEntries`
//! → empty → every auth header was silently dropped → the request failed its
//! `validateHeaders` check with "Could not resolve authentication method".
//!
//! Fix: `collection_iter_obj_for_receiver`
//! (`perry-runtime/src/array/iter_object.rs`) already routes Map/Set receivers
//! to their iterators; it now also routes fetch-band handles
//! (`Headers`/`FormData`/`URLSearchParams`) through `js_native_call_method` →
//! `js_headers_{keys,entries,values}`, which return a materialized, iterable
//! array. Non-collection fetch handles (Response/Request/Blob) and genuine
//! plain objects still fall through to the empty-array path.

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

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary exited non-zero: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).trim().to_string()
}

/// A `Headers` whose static type is erased through an object property / an
/// `any` boundary must still iterate via `.keys()`/`.entries()`/`.values()`
/// and `yield*` delegation — matching Node — not return an empty iterator.
#[test]
fn any_typed_headers_iterator_methods_return_entries() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = compile_and_run(
        dir.path(),
        r#"
function anyOf(x: any): any { return x; }
const h = new Headers();
h.append("x-api-key", "secret");
h.append("content-type", "application/json");
// Type erased through a wrapper property — the shape that dropped auth headers.
const z: any = anyOf({ values: h }).values;

const keys = [...z.keys()].sort();
const values = [...z.values()].sort();
let entryCount = 0;
for (const _e of z.entries()) entryCount++;
// `yield*` delegation over the any-typed entries (the exact SDK merge shape).
function* deleg(hh: any) { yield* hh.entries(); }
const delegCount = [...deleg(z)].length;

console.log(
  "keys=" + JSON.stringify(keys) +
  " values=" + JSON.stringify(values) +
  " entries=" + entryCount +
  " deleg=" + delegCount +
  " has=" + z.has("x-api-key")
);
"#,
    );
    assert_eq!(
        out,
        r#"keys=["content-type","x-api-key"] values=["application/json","secret"] entries=2 deleg=2 has=true"#,
    );
}
