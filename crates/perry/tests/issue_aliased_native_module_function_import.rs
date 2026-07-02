//! Aliased named imports of native-module functions must resolve to the
//! exported function, not the local alias.
//!
//! `import { join as p } from "path"; p(a, b)` returned `undefined` — the
//! native-module function-call lowering matched the LOCAL binding name (`p`)
//! against each module's known export names (`match func_name { "join" => … }`),
//! so an aliased local never matched and fell through to a generic path that
//! evaluated to `undefined`. Any later use then failed — e.g. the minified
//! config-dir shape `(process.env.X ?? p(home(), ".dir")).normalize("NFC")`
//! threw `Cannot read properties of undefined (reading 'normalize')`.
//! Unaliased `import { join }` worked only because local == export.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, entry: &std::path::Path) -> (bool, String) {
    let output = dir.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(entry)
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
    (
        run.status.success(),
        String::from_utf8_lossy(&run.stdout).to_string(),
    )
}

#[test]
fn aliased_native_module_function_imports_resolve() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
import { join as p, dirname as d, resolve as r } from "path"
import { homedir as h } from "os"

// Aliased path functions resolve to the real exports (not the local alias).
console.log("join:", p("/a", "b", "c"))
console.log("join-dot:", p("/a", ".dir"))
console.log("dirname:", d("/a/b/c"))
console.log("resolve-abs:", r("/a", "/b"))
console.log("homedir-type:", typeof h)

// The minified config-dir shape that surfaced the bug.
const cfg: any = (process.env.DEFINITELY_UNSET_CFG_DIR ?? p(h(), ".dir")).normalize("NFC")
console.log("cfg endsWith /.dir:", typeof cfg === "string" && cfg.endsWith("/.dir"))
console.log("DONE")
"#,
    )
    .expect("write entry");

    let (ok, stdout) = compile_and_run(dir.path(), &entry);
    assert!(ok, "binary failed\nstdout:\n{stdout}");
    for needle in [
        "join: /a/b/c",
        "join-dot: /a/.dir",
        "dirname: /a/b",
        "resolve-abs: /b",
        "homedir-type: function",
        "cfg endsWith /.dir: true",
        "DONE",
    ] {
        assert!(
            stdout.contains(needle),
            "expected `{needle}` in output:\n{stdout}"
        );
    }
}
