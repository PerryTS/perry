//! Regression test: `createRequire(...)(spec)` must resolve every implemented
//! Node built-in module, not just a subset.
//!
//! `tls` (and `dgram`/`domain`/`vm`/`repl`/`sqlite`/`inspector`) are fully
//! implemented native modules (runtime registry buckets + dispatch + exports),
//! but they were missing from the `supported_require_builtin` allowlist in
//! `module_require.rs`, so `require('tls')` via `createRequire` was rejected with
//! `ERR_PERRY_UNSUPPORTED_CREATE_REQUIRE` ("package/file require('tls') is not
//! supported"). This blocked `claude-code --help`: the bundled follow-redirects
//! module does `const tls = require('tls')` through a `createRequire` bridge.

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

    let run = Command::new(&output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn createrequire_resolves_tls_and_other_implemented_builtins() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import { createRequire } from "module";
const require = createRequire(import.meta.url);
const tls = require("tls");
console.log("tls:", typeof tls, typeof tls.createSecureContext === "function");
console.log("dgram:", typeof require("dgram"));
console.log("vm:", typeof require("vm"));
console.log("domain:", typeof require("domain"));
console.log("ok");
"#,
    );
    assert_eq!(
        stdout,
        "tls: object true\ndgram: object\nvm: object\ndomain: object\nok\n"
    );
}
