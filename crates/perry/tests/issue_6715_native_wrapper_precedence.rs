//! Issue #6715: a `perry.nativeLibrary` package whose `src/index.ts`
//! exports a REAL wrapper function whose name equals the derived ergonomic
//! alias (#5621) of one of its manifest symbols must run the wrapper — the
//! derived alias must NOT silently shadow it and bind the import straight to
//! the FFI symbol.
//!
//! The documented convention (`docs/src/plugins/native-extensions.md`) is an
//! ambient `declare function js_<pkg>_speak(...)` for the raw FFI symbol PLUS
//! a real `export function speak(...)` that transforms arguments and calls
//! the native function. When `<pkg>` happens to make the manifest symbol
//! follow `js_<pkg>_<snake>` (`js_foo_do_thing`), its derived alias
//! (`doThing`) collides with the wrapper's name. Before the fix Perry bound
//! the import to the FFI symbol, so the wrapper body was dead code and the
//! caller's arguments were passed raw to the native ABI — a silent failure
//! ("await never resolves") three layers from the cause.
//!
//! After the fix a genuine, implemented module export wins over the derived
//! alias. Ambient-declare-only packages (no body) keep the #5621 routing
//! (covered by `issue_5621_native_camel_export_routing.rs`).
//!
//! This test links a real one-function static archive (`js_foo_do_thing`
//! → `42`) via the manifest's `prebuilt` field, then asserts the compiled
//! program (1) prints the wrapper's side-effect marker and (2) prints the
//! wrapper's TRANSFORMED result (`42 + 1 == 43`) — neither is possible if
//! the import bound directly to the FFI symbol (which would print no marker
//! and yield the raw `42`).

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
fn real_wrapper_wins_over_derived_alias() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    // The native-library package: an ambient `declare` for the raw FFI
    // symbol PLUS a real `doThing` wrapper that logs a marker and transforms
    // the native return value. `doThing` collides with the derived alias of
    // `js_foo_do_thing` — the wrapper must win.
    let pkg_dir = root.join("node_modules/foo");
    std::fs::create_dir_all(pkg_dir.join("src")).expect("mkdir pkg src");

    let Some(_archive) = build_static_lib(&pkg_dir) else {
        eprintln!("skipping: no C toolchain (cc/ar) available");
        return;
    };

    std::fs::write(
        pkg_dir.join("src/index.ts"),
        r#"export declare function js_foo_do_thing(): number;

// Same name as the derived alias of `js_foo_do_thing` → "doThing".
export function doThing(): number {
  console.log("wrapper ran");
  return js_foo_do_thing() + 1;
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

    // Host project: allow the native library and import the wrapper API.
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
        "binary aborted\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // The wrapper's side effect proves it executed — under the bug the import
    // bound straight to the FFI symbol and this marker never printed.
    assert!(
        stdout.contains("wrapper ran"),
        "the wrapper body never ran — the derived alias shadowed the real \
         `doThing` export\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // The wrapper's transformed result (native 42 + 1) proves both that the
    // wrapper ran AND that it reached the native symbol internally. A raw
    // `42` here would mean the import bound directly to the FFI symbol.
    assert!(
        stdout.contains("result: 43"),
        "expected the wrapper's transformed result (43), not the raw FFI \
         value (42)\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
