//! Regression test for #9325: `WebSocketServer.clients` must be the stable,
//! iterable `Set` that npm `ws` exposes, even before any client connects.

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

fn compile_and_run(dir: &Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env_remove("PERRY_NO_AUTO_OPTIMIZE")
        .env("PERRY_WORKSPACE_ROOT", workspace_root())
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
fn websocket_server_clients_is_a_stable_iterable_set() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import { WebSocketServer } from "ws";

const wss = new WebSocketServer({ port: 0 });
const first = wss.clients;
const second = wss.clients;

console.log(Object.prototype.toString.call(first));
console.log(typeof first[Symbol.iterator]);
let count = 0;
for (const _client of first) count += 1;
console.log(first === second, count, first.size);
"#,
    );

    assert_eq!(stdout, "[object Set]\nfunction\ntrue 0 0\n");
}
