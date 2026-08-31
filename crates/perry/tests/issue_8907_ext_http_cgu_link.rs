//! Regression test for #8907.
//!
//! Reported as "any program importing node:http fails to link" on the released
//! 0.5.1220 macOS arm64 binary, with ~17 undefined `_js_ext_http_*` /
//! `_js_http_*` symbols referenced from `libperry_stdlib.a`. The node:http
//! framing is a red herring. The real trigger: prebuilt `libperry_stdlib.a`
//! packs a large slice of stdlib into one monolithic `cgu.0` object, and that
//! object *also* carries the unresolved `js_ext_http_*` references. Pulling the
//! member for any symbol it uniquely defines drags those references into the
//! link. Only a `node:http` import adds `libperry_ext_http.a` (which defines
//! them), so a program that touches `cgu.0` without importing `node:http`
//! links with the symbols unresolved.
//!
//! `new Blob(...)` is the minimal trigger: `js_blob_new` is defined only in
//! `cgu.0`. No http, no compiled package — one line reproduces it on 0.5.1220
//! and links on a fixed compiler. The original report hit it through a compiled
//! `effect` package, which happens to reference a `cgu.0`-only symbol.
//!
//! A link failure makes `perry compile` exit non-zero, so asserting the compile
//! succeeds is the test; running it confirms the member linked coherently.

use std::path::PathBuf;
use std::process::Command;

/// Path to the `perry` compiler binary built for this integration-test crate.
fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Compiles a program that references a `cgu.0`-only stdlib symbol (`Blob`)
/// with no `node:http` import, and asserts it links and runs. Guards the
/// #8907 regression where full-stdlib carried unresolved `js_ext_http_*`
/// references unless `node:http` pulled `perry-ext-http` onto the link line.
#[test]
fn cgu0_program_links_without_http_ext_archive() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let entry = root.join("blob.ts");
    // Blob pulls js_blob_new, which lives only in the cgu.0 object that carries
    // the ext-http references. No node:http import.
    std::fs::write(
        &entry,
        "const b = new Blob([\"x\"]); console.log(b.size);\n",
    )
    .expect("write blob.ts");
    let output = root.join("blob_bin");

    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "a Blob program with no node:http import failed to link — the #8907 \
         ext-http / cgu.0 coupling regressed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed at runtime\nstatus: {:?}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "1\n",
        "Blob([\"x\"]).size is 1 byte"
    );
}
