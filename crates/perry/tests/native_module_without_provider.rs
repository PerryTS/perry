//! A native module the compile's routing decision leaves without a provider
//! is a compile error, not a program that compiles and then fails at run time.
//!
//! With `PERRY_DISABLE_WELL_KNOWN=1`, `node:http` is not routed to its wrapper
//! crate perry-ext-http, and the bundled runtime does not implement
//! `createServer` (`NativeRouting::unprovided`). The same program compiles
//! with the wrapper routed. Both compiles use the same arguments; only the
//! setting differs.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const HTTP_PROGRAM: &str = r#"
import { createServer } from "node:http";

const server = createServer((req: any, res: any) => { res.end("ok"); });
console.log(typeof server.listen);
"#;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile(dir: &Path, disable_well_known: bool) -> Output {
    let entry = dir.join("main.ts");
    std::fs::write(&entry, HTTP_PROGRAM).expect("write entry");
    let mut command = Command::new(perry_bin());
    command
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(dir.join("main_bin"))
        .arg("--no-auto-optimize")
        .arg("--no-link")
        .env("PERRY_NO_CACHE", "1")
        .env_remove("PERRY_DISABLE_WELL_KNOWN")
        .env_remove("PERRY_FORCE_WELL_KNOWN");
    if disable_well_known {
        command.env("PERRY_DISABLE_WELL_KNOWN", "1");
    }
    command.output().expect("run perry compile")
}

#[test]
fn http_without_its_wrapper_is_a_compile_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = compile(dir.path(), true);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "compiling node:http with PERRY_DISABLE_WELL_KNOWN=1 must fail\nstdout:\n{}\nstderr:\n{stderr}",
        String::from_utf8_lossy(&output.stdout),
    );
    assert!(
        stderr.contains(
            "node:http has no provider: PERRY_DISABLE_WELL_KNOWN=1 disables its wrapper crate \
             perry-ext-http, and the bundled runtime doesn't implement it (the whole \
             module is refused under this setting). Unset PERRY_DISABLE_WELL_KNOWN to link \
             perry-ext-http."
        ),
        "the error must name the module, the setting, the wrapper crate and say the whole module is refused\nstderr:\n{stderr}"
    );
}

#[test]
fn http_with_its_wrapper_compiles() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = compile(dir.path(), false);
    assert!(
        output.status.success(),
        "compiling node:http with its wrapper routed must succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
