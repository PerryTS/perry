//! Issue #6714: a string that reaches JS through perry-ffi's
//! `JsPromise::resolve_string(...)` — which allocates via `js_string_from_bytes`
//! and NaN-boxes the result as `STRING_TAG` — must be `JSON.parse`-able like any
//! other JS string. It behaves normally for `typeof` / `.length` / indexing /
//! `.slice` / template literals; the report was that `JSON.parse` returned the
//! string **unparsed** (the input came back, no throw), so `Array.isArray(...)`
//! was `false` and `parsed[0]` was `"["`. The documented workaround was
//! `JSON.parse(s + "")`, which rebuilds the string through the runtime's own
//! concat allocator.
//!
//! Every native-library binding that returns JSON (the storekit-style contract —
//! "the native side returns a JSON string; the TypeScript wrapper parses it")
//! depends on this working. This test locks it in end-to-end.
//!
//! It links a real one-function static archive whose `js_jsonrepro_voices`
//! resolves a `Promise<string>` with a JSON array, exactly as
//! `JsPromise::resolve_string` does (`perry_ffi_promise_new` +
//! `js_string_from_bytes` NaN-boxed as `STRING_TAG` +
//! `perry_ffi_promise_resolve_bits`). The compiled program `await`s it and
//! `JSON.parse`s the result; the test asserts the parse produced a real array,
//! which is only possible if the FFI-allocated string parsed correctly.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn cc() -> String {
    std::env::var("CC").unwrap_or_else(|_| "cc".to_string())
}

/// Build `lib<name>.a` exporting `js_jsonrepro_voices`, which resolves a
/// `Promise<string>` with the JSON array `[{"a":1},{"a":2}]` via the same
/// primitives `perry-ffi`'s `JsPromise::resolve_string` uses. The runtime
/// symbols (`js_string_from_bytes`, `perry_ffi_promise_new`,
/// `perry_ffi_promise_resolve_bits`) are left undefined in the archive and get
/// resolved at final link against `libperry_runtime.a` / `libperry_stdlib.a` —
/// exactly how an external native-binding package links.
///
/// Returns `None` when the host lacks (or can't spawn) a C toolchain (`cc`/`ar`)
/// so the test skips gracefully rather than failing in toolchain-less
/// environments. Spawn failures are treated as "skip", not a test failure —
/// only an actually-broken compile/archive run is fatal.
fn build_static_lib(pkg_dir: &Path) -> Option<PathBuf> {
    let c_src = pkg_dir.join("voices.c");
    std::fs::write(
        &c_src,
        r#"#include <stdint.h>
#include <stddef.h>
#include <string.h>

extern void *js_string_from_bytes(const uint8_t *data, uint32_t len);
extern void *perry_ffi_promise_new(void);
extern void perry_ffi_promise_resolve_bits(void *promise, uint64_t bits);

/* Mirror of perry-ffi's JsPromise::resolve_string: alloc_string() ->
   js_string_from_bytes(); nanbox_string_bits() -> STRING_TAG | ptr;
   perry_ffi_promise_resolve_bits(). */
#define STRING_TAG   0x7FFF000000000000ULL
#define POINTER_MASK 0x0000FFFFFFFFFFFFULL

void *js_jsonrepro_voices(void) {
    void *p = perry_ffi_promise_new();
    const char *json = "[{\"a\":1},{\"a\":2}]";
    uint32_t len = (uint32_t)strlen(json);
    void *s = js_string_from_bytes((const uint8_t *)json, len);
    uint64_t bits = STRING_TAG | ((uint64_t)(uintptr_t)s & POINTER_MASK);
    perry_ffi_promise_resolve_bits(p, bits);
    return p;
}
"#,
    )
    .expect("write c source");
    let obj = pkg_dir.join("voices.o");
    let cc_out = Command::new(cc())
        .arg("-c")
        .arg(&c_src)
        .arg("-o")
        .arg(&obj)
        .output()
        .ok()?; // cc not spawnable → skip the test
                // A non-zero exit is a real failure, not a skip — surface it.
    assert!(
        cc_out.status.success(),
        "cc failed while building the test archive\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&cc_out.stdout),
        String::from_utf8_lossy(&cc_out.stderr)
    );
    let archive = pkg_dir.join("libvoices.a");
    let ar_out = Command::new("ar")
        .arg("rcs")
        .arg(&archive)
        .arg(&obj)
        .output()
        .ok()?; // ar not spawnable → skip the test
    assert!(
        ar_out.status.success(),
        "ar failed while building the test archive\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&ar_out.stdout),
        String::from_utf8_lossy(&ar_out.stderr)
    );
    Some(archive)
}

#[test]
fn json_parse_of_ffi_resolved_string_parses() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    // The native-library package: `voices()` over the `js_jsonrepro_voices`
    // FFI symbol that resolves a Promise<string> holding a JSON array.
    let pkg_dir = root.join("node_modules/jsonrepro");
    std::fs::create_dir_all(pkg_dir.join("src")).expect("mkdir pkg src");

    let Some(_archive) = build_static_lib(&pkg_dir) else {
        eprintln!("skipping: no C toolchain (cc/ar) available");
        return;
    };

    std::fs::write(
        pkg_dir.join("src/index.ts"),
        r#"declare function js_jsonrepro_voices(): Promise<string>;
export function voices(): Promise<string> {
  return js_jsonrepro_voices();
}
"#,
    )
    .expect("write pkg index.ts");

    std::fs::write(
        pkg_dir.join("package.json"),
        r#"{
  "name": "jsonrepro",
  "version": "1.0.0",
  "main": "src/index.ts",
  "types": "src/index.ts",
  "perry": {
    "nativeLibrary": {
      "abiVersion": "0.5",
      "functions": [
        { "name": "js_jsonrepro_voices", "params": [], "returns": "promise" }
      ],
      "targets": {
        "macos": { "prebuilt": "./libvoices.a" },
        "linux": { "prebuilt": "./libvoices.a" }
      }
    }
  }
}
"#,
    )
    .expect("write pkg package.json");

    // Host project: allow the native library.
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "host-app",
  "version": "1.0.0",
  "perry": { "allow": { "nativeLibrary": ["*"] } }
}
"#,
    )
    .expect("write host package.json");

    // `await` the FFI Promise<string>, JSON.parse it directly (no `+ ""`
    // workaround), and report whether the result is a real parsed array.
    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"import { voices } from "jsonrepro";

const s = await voices();
const parsed = JSON.parse(s);
console.log(
  "kind:", typeof s,
  "isArray:", Array.isArray(parsed),
  "len:", Array.isArray(parsed) ? parsed.length : -1,
  "first:", Array.isArray(parsed) ? parsed[0].a : "N/A",
);
"#,
    )
    .expect("write entry");

    let output_bin = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .env("PERRY_ALLOW_PERRY_FEATURES", "1")
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output_bin)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output_bin)
        .output()
        .expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        run.status.success(),
        "binary aborted\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // The bug: JSON.parse returned the string unparsed → `isArray: false`,
    // `first: N/A`. The fix: a real 2-element array parsed from the
    // FFI-allocated string.
    assert!(
        stdout.contains("kind: string")
            && stdout.contains("isArray: true")
            && stdout.contains("len: 2")
            && stdout.contains("first: 1"),
        "JSON.parse of an FFI-resolved string did not parse to an array \
         (issue #6714 regression)\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
