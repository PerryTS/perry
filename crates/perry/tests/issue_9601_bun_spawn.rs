//! End-to-end coverage for Bun.spawn and Bun.Terminal (#9601).

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
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
        .arg("--platform")
        .arg("bun")
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
        "compiled binary failed ({:?})\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn bun_spawn_streams_stdio_lifecycle_and_errors() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import { spawn } from "bun";
import { closeSync, openSync, readFileSync } from "node:fs";

async function main() {
  const basic = spawn([process.execPath, "-e", 'process.stdout.write("ok")'], {
    stdin: "ignore",
    stdout: "pipe",
    stderr: "pipe",
    cwd: process.cwd(),
    env: process.env,
    windowsHide: true,
  });
  basic.unref();
  basic.ref();
  const basicText = await basic.stdout.text();
  const basicExit = await basic.exited;
  console.log("BASIC:" + basicText + ":" + basicExit + ":" + typeof basic.pid);
  console.log("STREAMS:" + ["text", "json", "arrayBuffer", "bytes"].every((key) => typeof basic.stdout[key] === "function"));

  let callback = "missing";
  await using objectChild = spawn({
    cmd: ["/bin/sh", "-lc", "printf object-ok"],
    stdout: "pipe",
    stderr: "pipe",
    onExit(_child, code, signal, error) {
      callback = code + ":" + signal + ":" + error;
    },
  });
  console.log("OBJECT:" + await objectChild.stdout.text() + ":" + await objectChild.exited + ":" + callback);

  const argvChild = spawn(["/bin/sh", "-c", 'printf %s "$0"'], {
    argv0: "custom-argv0",
    stdout: "pipe",
    stderr: "pipe",
  });
  console.log("ARGV0:" + await argvChild.stdout.text() + ":" + await argvChild.exited);

  const fdPath = process.cwd() + "/fd-output.txt";
  const fd = openSync(fdPath, "w");
  const fdChild = spawn(["/bin/sh", "-lc", "printf fd-ok"], {
    stdin: "ignore",
    stdout: fd,
    stderr: "pipe",
  });
  await fdChild.exited;
  closeSync(fd);
  console.log("FD:" + readFileSync(fdPath, "utf8"));

  const filePath = process.cwd() + "/bun-file-output.txt";
  const fileChild = spawn(["/bin/sh", "-lc", "printf file-ok"], {
    stdin: "ignore",
    stdout: Bun.file(filePath),
    stderr: "pipe",
  });
  await fileChild.exited;
  console.log("FILE:" + await Bun.file(filePath).text());

  const killedChild = spawn(["/bin/sh", "-lc", "sleep 30"], {
    stdin: "ignore",
    stdout: "ignore",
    stderr: "ignore",
    detached: true,
  });
  const delivered = killedChild.kill("SIGTERM");
  const killedExit = await killedChild.exited;
  console.log("KILL:" + delivered + ":" + killedChild.killed + ":" + typeof killedExit);

  try {
    spawn(["/definitely/missing/perry-bun-spawn"]);
  } catch (error: any) {
    console.log("ERROR:" + error.code);
  }
}

main();
"#,
    );

    for expected in [
        "BASIC:ok:0:number",
        "STREAMS:true",
        "OBJECT:object-ok:0:0:null:undefined",
        "ARGV0:custom-argv0:0",
        "FD:fd-ok",
        "FILE:file-ok",
        "KILL:true:true:number",
        "ERROR:ENOENT",
    ] {
        assert!(
            stdout.contains(expected),
            "missing {expected:?} in {stdout:?}"
        );
    }
}

#[test]
fn bun_terminal_attaches_a_posix_pty() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import { spawn, Terminal } from "bun";

async function main() {
  let output = "";
  let exitSeen = "missing";
  let drains = 0;
  await using globalTerminal = new Bun.Terminal();
  console.log("GLOBAL-TERMINAL:" + typeof globalTerminal.resize);
  await using terminal = new Terminal({
    cols: 80,
    rows: 24,
    data(_terminal, bytes) {
      output += Buffer.from(bytes).toString();
    },
    exit(_terminal, code, signal, error) {
      exitSeen = code + ":" + signal + ":" + error;
    },
    drain() {
      drains++;
    },
  });

  const child = spawn(["/bin/sh", "-lc", "printf pty-ok"], { terminal });
  terminal.write("");
  terminal.setRawMode(true);
  terminal.setRawMode(false);
  terminal.unref();
  terminal.ref();
  child.unref();
  child.ref();
  const code = await child.exited;
  terminal.resize(100, 30);

  console.log("PTY:" + output.includes("pty-ok") + ":" + code + ":" + typeof child.pid);
  console.log("SIZE:" + terminal.cols + "x" + terminal.rows);
  console.log("CALLBACKS:" + exitSeen + ":" + drains);
  console.log("METHODS:" + ["write", "resize", "setRawMode", "ref", "unref", "close"].every((key) => typeof terminal[key] === "function"));

  const killed = spawn(["/bin/sh", "-lc", "sleep 30"], { terminal });
  killed.kill("SIGTERM");
  const killedCode = await killed.exited;
  console.log("PTY-KILL:" + killed.exitCode + ":" + killed.signalCode + ":" + typeof killedCode);
}

main();
"#,
    );

    for expected in [
        "GLOBAL-TERMINAL:function",
        "PTY:true:0:number",
        "SIZE:100x30",
        "CALLBACKS:0:null:undefined:1",
        "METHODS:true",
        "PTY-KILL:null:SIGTERM:number",
    ] {
        assert!(
            stdout.contains(expected),
            "missing {expected:?} in {stdout:?}"
        );
    }
}
