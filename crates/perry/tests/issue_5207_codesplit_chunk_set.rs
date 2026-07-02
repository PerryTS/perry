//! Regression test for #5207: perry ingests a bundler's code-split chunk-set
//! (an entry chunk plus a set of `import("./chunk-….js")` chunks) and compiles
//! the whole graph to native code.
//!
//! The concrete blocker this guards: `bun build --splitting` / esbuild emit
//! every chunk with a leading banner comment immediately followed by a
//! top-level `export`/`import`. The `.js` module-vs-script detector
//! (`looks_like_es_module`) used to read the banner comment's last character as
//! if it were code, decide the `export` could not begin a module item, and
//! parse the chunk as a Script — SWC then raised `ImportExportInScript` and the
//! whole build failed. The chunk-set below exercises:
//!   * the entry chunk and every code-split chunk opening with a banner comment,
//!   * a statically-knowable `import("./chunk-….js")` per route (compiled in,
//!     not deferred),
//!   * a shared chunk imported statically by the entry AND by the route chunks
//!     (dedup), and
//!   * a cross-chunk re-export (`export { … } from "./chunk-runtime.js"`).
//! The compiled binary's output must match Node's byte-for-byte.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

fn target_debug_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"))
        .join("debug")
}

/// Build `libperry_runtime.a` / `libperry_stdlib.a` once so the compiled binary
/// can link. Since #5422 runtime/stdlib are rlib-only; the staticlibs come from
/// the `perry-{runtime,stdlib}-static` wrapper crates. The CI `cargo-test` job
/// doesn't pre-build them.
fn ensure_runtime_archive() {
    static BUILD_RUNTIME: Once = Once::new();
    BUILD_RUNTIME.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let build = Command::new(cargo)
            .current_dir(workspace_root())
            .arg("build")
            .arg("-p")
            .arg("perry-runtime-static")
            .arg("-p")
            .arg("perry-stdlib-static")
            .output()
            .expect("run cargo build for static wrapper crates");
        assert!(
            build.status.success(),
            "cargo build -p perry-runtime-static -p perry-stdlib-static failed\n\
             stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
    });
}

fn runtime_dir() -> PathBuf {
    ensure_runtime_archive();
    target_debug_dir()
}

// --- the chunk-set, shaped like a `bun build --splitting` output ---

const CHUNK_SHARED: &str = "\
// chunk-shared.js — shared chunk imported by the entry and by route chunks
export const VERSION = \"1.0.0\";
export function fmt(name) { return `[${VERSION}] ${name}`; }
";

const CHUNK_RUNTIME: &str = "\
// chunk-runtime.js — runtime chunk re-exported by a route chunk (cross-chunk re-export)
export function runtimeTag(x) { return \"rt:\" + x; }
";

const CHUNK_ROUTE_A: &str = "\
// chunk-route-a.js (code-split route)
import { fmt } from \"./chunk-shared.js\";
export function handler() { return fmt(\"route-a\"); }
";

const CHUNK_ROUTE_B: &str = "\
// chunk-route-b.js (code-split route, cross-chunk re-export)
import { fmt } from \"./chunk-shared.js\";
export { runtimeTag } from \"./chunk-runtime.js\";
import { runtimeTag } from \"./chunk-runtime.js\";
export function handler() { return fmt(\"route-b\") + \" \" + runtimeTag(\"b\"); }
";

const ENTRY: &str = "\
// entry.js (bundle entry; opens with a banner comment like every chunk)
import { VERSION } from \"./chunk-shared.js\";

async function loadRoute(name) {
  if (name === \"a\") {
    const m = await import(\"./chunk-route-a.js\");
    return m.handler();
  } else {
    const m = await import(\"./chunk-route-b.js\");
    return m.handler() + \" \" + m.runtimeTag(\"re\");
  }
}

async function main() {
  console.log(\"entry version\", VERSION);
  console.log(await loadRoute(\"a\"));
  console.log(await loadRoute(\"b\"));
}
main();
";

const EXPECTED: &str = "entry version 1.0.0\n[1.0.0] route-a\n[1.0.0] route-b rt:b rt:re\n";

fn write_chunk_set(root: &std::path::Path) {
    std::fs::write(root.join("chunk-shared.js"), CHUNK_SHARED).unwrap();
    std::fs::write(root.join("chunk-runtime.js"), CHUNK_RUNTIME).unwrap();
    std::fs::write(root.join("chunk-route-a.js"), CHUNK_ROUTE_A).unwrap();
    std::fs::write(root.join("chunk-route-b.js"), CHUNK_ROUTE_B).unwrap();
    std::fs::write(root.join("entry.js"), ENTRY).unwrap();
}

#[test]
fn codesplit_chunk_set_compiles_and_runs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_chunk_set(root);

    let entry = root.join("entry.js");
    let output = root.join("entry_bin");
    let out = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .output()
        .expect("run perry compile");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "compiling a banner-commented code-split chunk-set must succeed; stderr:\n{stderr}"
    );
    // The pre-fix failure mode was a hard parse error on a chunk.
    assert!(
        !stderr.contains("ImportExportInScript"),
        "a banner-commented `.js` chunk must parse as a module, not a script; stderr:\n{stderr}"
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled chunk-set binary must run; stderr:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        stdout, EXPECTED,
        "chunk-set output must match Node byte-for-byte; got:\n{stdout}"
    );
}
