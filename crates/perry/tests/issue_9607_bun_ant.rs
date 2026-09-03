//! #9607 — Claude Code's vendor Bun.ant peer-credential and pressure hooks.

#![cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile(dir: &Path) -> PathBuf {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg("--platform")
        .arg("bun")
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
    output
}

#[test]
fn bun_ant_reads_unix_socket_peers_and_handles_failures_conservatively() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("main.ts"),
        r#"
import * as net from "node:net";
import { closeSync, openSync } from "node:fs";

const socketPath = process.cwd() + "/peer.sock";
let client: any;
const lines = await new Promise<string[]>((resolve, reject) => {
  const output: string[] = [];
  const server = net.createServer((socket: any) => {
    const fd = socket._handle?.fd;
    output.push("fd " + (typeof fd === "number" && fd >= 0));
    output.push("uid " + (Bun.ant.getPeerUid(fd) === process.getuid()));
    const pid = Bun.ant.getPeerPid(fd);
    output.push("pid " + (pid === null || pid > 0));
    output.push("invalid " + (Bun.ant.getPeerUid(-1) === null && Bun.ant.getPeerPid(-1) === null));

    const closed = openSync(process.cwd() + "/closed-fd", "w");
    closeSync(closed);
    output.push("closed " + (Bun.ant.getPeerUid(closed) === null && Bun.ant.getPeerPid(closed) === null));

    socket.destroy();
    client.destroy();
    server.close();
    resolve(output);
  });
  server.once("error", reject);
  server.listen(socketPath, () => {
    client = net.connect({ path: socketPath });
    client.once("error", reject);
    client.resume();
  });
});

for (const line of lines) console.log(line);
console.log("namespace " + (typeof Bun.ant === "object" && Bun.ant === Bun.ant));
console.log("methods " + Object.keys(Bun.ant).sort().join(","));
const pressure = Bun.ant.memoryPressureLevel();
console.log("pressure " + (pressure === null || pressure === "normal" || pressure === "warning" || pressure === "critical"));
"#,
    )
    .expect("write entry");

    let output = compile(dir.path());
    let run = Command::new(&output)
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "fd true\nuid true\npid true\ninvalid true\nclosed true\nnamespace true\nmethods getPeerPid,getPeerUid,memoryPressureLevel\npressure true\n"
    );
}
