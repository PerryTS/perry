//! `new <undici namespace>.Agent(...)` must construct the native undici agent,
//! as the bare `new Agent(...)` import already does.
//!
//! Only the bare-identifier form (`import { Agent } from "undici"`) was routed
//! to the native constructor. esbuild's CommonJS output
//! (`var import_undici = require("undici"); new import_undici.Agent(...)`) and
//! `import * as undici` reached a dynamic `new` on the module's `Agent`
//! property, which is not a constructor: module init threw "undefined is not a
//! constructor". The programs below are compiled and run, so a regression shows
//! as a failed start instead of a lowering shape.

use std::path::{Path, PathBuf};
use std::process::Command;

fn compile_and_run(root: &Path, entry: &str, source: &str) -> String {
    std::fs::write(root.join(entry), source).expect("write entry");
    let compiler = PathBuf::from(env!("CARGO_BIN_EXE_perry"));
    let runtime = std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| compiler.parent().unwrap().to_owned());
    let output = root.join(format!("{entry}.bin"));
    let compile = Command::new(compiler)
        .current_dir(root)
        .args(["compile", entry, "--no-cache", "--no-auto-optimize"])
        .arg("-o")
        .arg(&output)
        .env("PERRY_RUNTIME_DIR", runtime)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "{entry}: perry compile failed\n{}\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "{entry}: compiled binary failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8(run.stdout).expect("utf8 stdout")
}

#[test]
fn required_undici_namespace_constructs_agents() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        "entry.cjs",
        r#"var import_undici = require("undici");
const agent = new import_undici.Agent({ keepAliveTimeout: 1000 });
const proxy = new import_undici.ProxyAgent("http://127.0.0.1:1");
console.log(typeof agent, agent !== null, typeof proxy, proxy !== null);
"#,
    );
    assert_eq!(stdout, "object true object true\n");
}

#[test]
fn star_imported_undici_namespace_constructs_agents() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        "entry.mjs",
        r#"import * as undici from "undici";
const agent = new undici.Agent({ keepAliveTimeout: 1000 });
console.log(typeof agent, agent !== null);
"#,
    );
    assert_eq!(stdout, "object true\n");
}
