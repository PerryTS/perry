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
