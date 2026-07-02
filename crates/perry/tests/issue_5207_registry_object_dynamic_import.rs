//! Regression test for #5207 (registry-object follow-up): a const object
//! literal that maps route/feature keys to statically-knowable chunk paths is
//! a *registry*, and `import(REGISTRY[runtimeKey])` / `import(REGISTRY.key)`
//! must ingest the whole chunk set rather than deferring to a runtime error.
//!
//! Bundlers and hand-written lazy-load tables routinely centralize their
//! `import("./chunk-….js")` targets in such a table and index it with a
//! runtime-computed key. The targets are still fully known at build time, so
//! perry over-approximates the member access to the union of the registry's
//! (relative) value specifiers and compiles every one. The runtime `import()`
//! dispatch then picks the right chunk by path string.
//!
//! Guard rail also asserted: a plain data object whose values are *not*
//! relative module specifiers (`{ name: "app", port: "3000" }`) indexed for a
//! non-import reason must keep deferring — it must never try to compile
//! `"app"` / `"3000"` as modules.

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

/// Build the static runtime/stdlib archives once so the compiled binary links.
/// Mirrors `issue_5207_codesplit_chunk_set.rs`.
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

const CHUNK_A: &str = "export function h() { return \"chunk-a\"; }\n";
const CHUNK_B: &str = "export function h() { return \"chunk-b\"; }\n";
const CHUNK_C: &str = "export function h() { return \"chunk-c\"; }\n";

// Entry maps three route keys to chunk paths through a const registry, then
// loads them by a *runtime-computed* key (the `import()` arg is `REGISTRY[k]`,
// never a literal). A static-property access (`REGISTRY.c`) is exercised too.
const ENTRY: &str = "\
const REGISTRY = {
  a: \"./chunk-a.js\",
  b: \"./chunk-b.js\",
  c: \"./chunk-c.js\",
};

async function load(key) {
  const m = await import(REGISTRY[key]);
  return m.h();
}

async function main() {
  for (const k of [\"a\", \"b\"]) {
    console.log(k, await load(k));
  }
  // static member access into the same registry
  const m = await import(REGISTRY.c);
  console.log(\"c\", m.h());
}
main();
";

const EXPECTED: &str = "a chunk-a\nb chunk-b\nc chunk-c\n";

#[test]
fn registry_object_dynamic_import_compiles_chunk_set() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("chunk-a.js"), CHUNK_A).unwrap();
    std::fs::write(root.join("chunk-b.js"), CHUNK_B).unwrap();
    std::fs::write(root.join("chunk-c.js"), CHUNK_C).unwrap();
    std::fs::write(root.join("entry.js"), ENTRY).unwrap();

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
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "registry-object dynamic import must compile; stderr:\n{stderr}"
    );
    // The registry targets must be ingested, not deferred to a runtime error.
    assert!(
        !stdout.contains("deferred runtime error") && !stderr.contains("deferred runtime error"),
        "registry chunk targets must compile in, not defer; stdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled registry binary must run; stderr:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        EXPECTED,
        "registry-object dynamic-import output must match Node byte-for-byte"
    );
}

// A non-module data object indexed by a runtime key must NOT be mistaken for a
// chunk registry: its values aren't relative specifiers, so the site stays
// deferred (compiles, throws only if reached) instead of trying to compile
// `"app"` / `"3000"` as modules and breaking the build.
const NON_REGISTRY_ENTRY: &str = "\
const cfg = { name: \"app\", port: \"3000\" };
async function main() {
  const k = \"name\";
  try {
    await import(cfg[k]);
    console.log(\"unexpected\");
  } catch (e) {
    console.log(\"caught\");
  }
}
main();
";

#[test]
fn non_module_data_object_indexed_stays_deferred() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("entry.js"), NON_REGISTRY_ENTRY).unwrap();

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

    // Must still build (deferred, not a hard error) — the registry heuristic
    // must not have tried to resolve "app"/"3000" as modules.
    assert!(
        out.status.success(),
        "a non-module data object indexed by a runtime key must still compile (deferred); \
         stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(run.status.success(), "compiled binary must run");
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "caught\n",
        "the deferred import() must throw at runtime, matching Node's import(undefined-ish)"
    );
}
