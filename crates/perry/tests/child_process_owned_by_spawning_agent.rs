//! A child process belongs to the agent that spawned it. Perry's live-child
//! registry and child event queue are process-global, and every agent's pump
//! used to drain them: the main thread took a worker's child events, emitted
//! `spawn`/`exit`/`close` on the worker's ChildProcess from the wrong thread,
//! and the worker never saw them. opencode's server runs in a Worker and runs
//! `git rev-parse` before its first response, so inside any git repository the
//! TUI never drew its first frame.
//!
//! The fixture spawns one child from a worker and one from the main thread at
//! the same time; each side must see its own child's full lifecycle and output.

#![cfg(unix)]

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MAIN: &str = r#"
import { spawn } from "child_process";
const seen: string[] = [];
let workerDone = false;
let mainDone = false;
const finish = () => {
  if (!workerDone || !mainDone) return;
  console.log(seen.join("\n"));
  console.log("OK");
  process.exit(0);
};
const w = new Worker(new URL("./cp_worker.ts", import.meta.url));
w.onmessage = (e: any) => {
  const msg = String(e.data);
  seen.push("worker " + msg);
  if (msg.startsWith("close")) { workerDone = true; finish(); }
};
const p = spawn("/bin/sh", ["-c", "echo main-out; exit 4"], { stdio: ["ignore", "pipe", "pipe"] });
let out = "";
p.on("spawn", () => seen.push("main spawn"));
p.stdout!.on("data", (d: any) => { out += d.toString(); });
p.on("exit", (c: any) => seen.push("main exit " + c));
p.on("close", (c: any) => { seen.push("main close " + c + " " + out.trim()); mainDone = true; finish(); });
setTimeout(() => { console.log(seen.join("\n")); console.log("TIMEOUT"); process.exit(1); }, 10000);
"#;

const WORKER: &str = r#"
import { spawn } from "child_process";
declare var self: any;
const post = (s: string) => self.postMessage(s);
const p = spawn("/bin/sh", ["-c", "echo worker-out; echo worker-err >&2; exit 3"], {
  stdio: ["ignore", "pipe", "pipe"],
  detached: true,
});
let out = "";
let err = "";
p.on("spawn", () => post("spawn"));
p.stdout!.on("data", (d: any) => { out += d.toString(); });
p.stderr!.on("data", (d: any) => { err += d.toString(); });
p.on("exit", (c: any) => post("exit " + c));
p.on("close", (c: any) => post("close " + c + " " + out.trim() + " " + err.trim()));
"#;

fn compile(dir: &Path) -> std::path::PathBuf {
    let source = dir.join("cp_main.ts");
    let binary = dir.join("cp_main_bin");
    std::fs::write(&source, MAIN).expect("write main fixture");
    std::fs::write(dir.join("cp_worker.ts"), WORKER).expect("write worker fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(dir)
        .arg("compile")
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("compile fixture");
    assert!(
        output.status.success(),
        "fixture compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    binary
}

#[test]
fn a_workers_child_process_events_reach_the_worker() {
    let dir = tempfile::tempdir().expect("create fixture directory");
    let binary = compile(dir.path());
    let mut child = Command::new(&binary)
        .current_dir(dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run fixture");
    let started = Instant::now();
    while child.try_wait().expect("poll fixture").is_none() {
        if started.elapsed() > Duration::from_secs(30) {
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let output = child.wait_with_output().expect("collect fixture output");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    for want in [
        "worker spawn",
        "worker exit 3",
        "worker close 3 worker-out worker-err",
        "main spawn",
        "main exit 4",
        "main close 4 main-out",
        "OK",
    ] {
        assert!(
            lines.contains(&want),
            "missing `{want}`\nstdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let order = |s: &str| lines.iter().position(|l| *l == s).unwrap();
    assert!(order("worker spawn") < order("worker exit 3"));
    assert!(order("worker exit 3") < order("worker close 3 worker-out worker-err"));
}

/// `close` comes one pump after `exit`, and nothing woke the loop for that
/// pump: when a child's exit and both pipe EOFs landed in one pump, the loop
/// parked until its idle cap and every such child closed a full second late.
/// The fixture makes that ordering certain by blocking the loop until the
/// child is done; Node closes each in ~100 ms (the block itself).
const CLOSE_TURN: &str = r#"
import { spawn } from "child_process";
const run = (): Promise<number> => new Promise((res) => {
  const p = spawn("/bin/echo", ["hi"], { stdio: ["ignore", "pipe", "pipe"] });
  const t = Date.now();
  while (Date.now() - t < 100) {}
  p.stdout!.on("data", () => {});
  p.on("close", () => res(Date.now() - t));
});
(async () => {
  const ms: number[] = [];
  for (let i = 0; i < 5; i++) ms.push(await run());
  console.log("close_ms " + ms.join(","));
  process.exit(0);
})();
"#;

#[test]
fn close_follows_exit_without_waiting_for_an_unrelated_wake() {
    let dir = tempfile::tempdir().expect("create fixture directory");
    let source = dir.path().join("close_turn.ts");
    let binary = dir.path().join("close_turn_bin");
    std::fs::write(&source, CLOSE_TURN).expect("write fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(dir.path())
        .arg("compile")
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("compile fixture");
    assert!(output.status.success(), "fixture compile failed");
    let run = Command::new(&binary).output().expect("run fixture");
    let stdout = String::from_utf8_lossy(&run.stdout);
    let line = stdout
        .lines()
        .find_map(|l| l.strip_prefix("close_ms "))
        .unwrap_or_else(|| panic!("no close_ms line in {stdout:?}"));
    for ms in line.split(',') {
        let ms: u64 = ms.parse().expect("close time in ms");
        assert!(ms < 600, "a child closed {ms} ms after spawn (all: {line})");
    }
}
