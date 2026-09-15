//! Regression test for #10302: `dlopen` on an embedded `$perryfs/` asset path.
//!
//! `import lib from "./libfoo.so" with { type: "file" }` lowers to a
//! `$perryfs/<name>` virtual path served by `crate::embedded`. `crate::fs`
//! understands those paths; the dynamic loader does not — it needs a real
//! filesystem path — so `dlopen(lib)` failed with "cannot open shared object
//! file". `@opentui/core` ships its renderer exactly this way, which is how
//! OpenCode's TUI starts (#10107).
//!
//! The fix materializes the embedded bytes into a temp file once per virtual
//! path, inside `open_library` — the single choke point both `dlopen_value`
//! (bun:ffi) and `node_dlopen_value` (`process.dlopen`) reach. Patching only
//! the first left the second broken, which is why the translation lives there
//! and not at either entry point.
//!
//! The control compiles the same program against the dylib's REAL path, so a
//! failure in the embedded case cannot be blamed on the fixture or the host's
//! `cc`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            perry_bin()
                .parent()
                .expect("compiler directory")
                .to_path_buf()
        })
}

const LIB_C: &str = "int perry_embedded_answer(void) { return 4242; }\n";

fn lib_file_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "libperryembed.dylib"
    } else {
        "libperryembed.so"
    }
}

/// Build the fixture with the system `cc` — the same toolchain the perry
/// driver links with. Returns `None` when `cc` is unavailable so an offline or
/// toolchain-less runner skips instead of failing.
fn build_fixture(dir: &Path) -> Option<PathBuf> {
    let c_path = dir.join("perryembed.c");
    std::fs::write(&c_path, LIB_C).expect("write C fixture");
    let lib_path = dir.join(lib_file_name());
    let out = Command::new("cc")
        .current_dir(dir)
        .arg("-shared")
        .arg("-fPIC")
        .arg("-o")
        .arg(&lib_path)
        .arg(&c_path)
        .output()
        .ok()?;
    if !out.status.success() {
        eprintln!(
            "skipping: cc could not build the fixture dylib:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        return None;
    }
    Some(lib_path)
}

fn entry_source(embedded: bool) -> String {
    let path_binding = if embedded {
        format!(
            "import libPath from \"./{}\" with {{ type: \"file\" }}",
            lib_file_name()
        )
    } else {
        format!(
            "const libPath = new URL(\"./{}\", import.meta.url).pathname",
            lib_file_name()
        )
    };
    format!(
        r#"{path_binding}
import {{ dlopen, FFIType }} from "bun:ffi"

const {{ symbols }} = dlopen(libPath, {{
  perry_embedded_answer: {{ args: [], returns: FFIType.i32 }},
}})
console.log("answer:", symbols.perry_embedded_answer())
"#
    )
}

fn compile_and_run(dir: &Path, source: &str) -> (bool, String, String) {
    let entry = dir.join("main.ts");
    std::fs::write(&entry, source).expect("write entry");
    let output = dir.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("--platform")
        .arg("bun")
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&output).output().expect("run compiled binary");
    (
        run.status.success(),
        String::from_utf8_lossy(&run.stdout).to_string(),
        String::from_utf8_lossy(&run.stderr).to_string(),
    )
}

#[test]
fn dlopen_reaches_an_embedded_asset_library() {
    let dir = tempfile::tempdir().expect("tempdir");
    let Some(_lib) = build_fixture(dir.path()) else {
        return;
    };

    let (ok, stdout, stderr) = compile_and_run(dir.path(), &entry_source(true));
    assert!(
        ok,
        "dlopen of an embedded $perryfs path must succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("cannot open shared object file"),
        "the loader must never see the virtual path\nstderr:\n{stderr}"
    );
    assert_eq!(
        stdout, "answer: 4242\n",
        "the embedded library's symbol must return its real value"
    );
}

#[test]
fn real_path_control() {
    let dir = tempfile::tempdir().expect("tempdir");
    let Some(_lib) = build_fixture(dir.path()) else {
        return;
    };

    let (ok, stdout, stderr) = compile_and_run(dir.path(), &entry_source(false));
    assert!(
        ok,
        "the control must load the same dylib by its real path\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert_eq!(stdout, "answer: 4242\n", "control output");
}
