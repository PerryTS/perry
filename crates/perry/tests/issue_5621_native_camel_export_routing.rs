//! Issue #5621: a `perry.nativeLibrary` package that exposes ergonomic
//! camelCase exports (`doThing`) over snake_case `js_<pkg>_*` FFI symbols
//! (`js_foo_do_thing`) must route the call to the native symbol — NOT run
//! the throwing TypeScript wrapper body.
//!
//! Before the fix, Perry only routed a native-library import to its FFI
//! symbol when the import binding was byte-for-byte equal to the manifest
//! symbol name (the `@perryts/storekit` raw-ambient-export convention). An
//! ergonomic camelCase binding never matched, so the call silently fell
//! through to the package's `.ts` body (here: a `throw`).
//!
//! This test links a real one-function static archive (`js_foo_do_thing`
//! → `42`) via the manifest's `prebuilt` field, then asserts the compiled
//! program prints `42` — which is only possible if `doThing()` reached the
//! native symbol rather than the throwing wrapper.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn cc() -> String {
    std::env::var("CC").unwrap_or_else(|_| "cc".to_string())
}

/// Build `lib<name>.a` exporting `int64_t js_foo_do_thing(void) { return 42; }`.
/// Returns `None` when the host lacks (or can't spawn) a C toolchain
/// (`cc`/`ar`), so the test skips gracefully rather than failing in
/// toolchain-less environments. Spawn failures are treated as "skip", not a
/// test failure — only an actually-broken compile/archive run is fatal.
fn build_static_lib(pkg_dir: &Path) -> Option<PathBuf> {
    let c_src = pkg_dir.join("foo.c");
    std::fs::write(
        &c_src,
        "#include <stdint.h>\nint64_t js_foo_do_thing(void) { return 42; }\n",
    )
    .expect("write c source");
    let obj = pkg_dir.join("foo.o");
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
    let archive = pkg_dir.join("libfoo.a");
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
fn camel_case_native_export_routes_to_ffi_symbol() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    // The native-library package: camelCase `doThing` surface over the
    // `js_foo_do_thing` FFI symbol. The body THROWS — if Perry runs it,
    // the program aborts instead of printing 42, which is exactly the bug.
    let pkg_dir = root.join("node_modules/foo");
    std::fs::create_dir_all(pkg_dir.join("src")).expect("mkdir pkg src");

    let Some(_archive) = build_static_lib(&pkg_dir) else {
        eprintln!("skipping: no C toolchain (cc/ar) available");
        return;
    };

    std::fs::write(
        pkg_dir.join("src/index.ts"),
        r#"export function doThing(): number {
  throw new Error("native only — must route to js_foo_do_thing");
}
"#,
    )
    .expect("write pkg index.ts");

    std::fs::write(
        pkg_dir.join("package.json"),
        r#"{
  "name": "foo",
  "version": "1.0.0",
  "main": "src/index.ts",
  "types": "src/index.ts",
  "perry": {
    "nativeLibrary": {
      "abiVersion": "0.5",
      "functions": [
        { "name": "js_foo_do_thing", "params": [], "returns": "i64" }
      ],
      "targets": {
        "macos": { "prebuilt": "./libfoo.a" },
        "linux": { "prebuilt": "./libfoo.a" }
      }
    }
  }
}
"#,
    )
    .expect("write pkg package.json");

    // Host project: allow the native library and import the camelCase API.
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

    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"import { doThing } from "foo";
console.log("result:", doThing());
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
        "binary aborted — the camelCase export ran its throwing TS body \
         instead of routing to js_foo_do_thing\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("result: 42"),
        "expected the native FFI symbol's return value (42)\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
