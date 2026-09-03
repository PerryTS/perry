//! Regression test for #9591: `fs.watch` must not re-walk its target on a
//! timer.
//!
//! Before the fix every watcher was a 25 ms `setInterval` whose tick walked
//! the WHOLE watch target (`read_dir` + `symlink_metadata` per entry) and
//! diffed two maps, on the main thread — ~3.4 µs per file per tick, 40 ticks
//! a second. Measured over 5 s of watching 3 000 files: 2.03 s of CPU (41 %
//! of a core) against Node's 0.03 s. claude-code watches its cwd; a 362 k-file
//! cwd extrapolates to ~1.2 s of walking per 25 ms schedule, i.e. a wedged
//! event loop.
//!
//! The fix hands change detection to the OS (`notify`: inotify / FSEvents /
//! ReadDirectoryChangesW) and keeps the walker only as an off-main-thread
//! fallback paced to 5 % of a core. This test is the issue's verification
//! bar: watch 3 000 files for a window, assert the process burned less than
//! 5 % of a core doing it, AND that a change is still seen promptly — both
//! halves matter, since a poller with a long enough interval would pass the
//! CPU bound alone. The unfixed walker burns ~1.6 s in this window; the
//! native backend burns a few milliseconds.
//!
//! The second test forces the fallback (`PERRY_FS_WATCH_POLL=1`) and checks
//! that it, too, respects the budget: at 3 000 files a walk is ~10 ms, so the
//! adaptive interval sits near 200 ms and the duty cycle at 5 %.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const FILE_COUNT: usize = 3_000;
const DIR_COUNT: usize = 30;
const WINDOW_MS: u64 = 4_000;

/// Watches argv[2] recursively, reports every event, and after argv[3] ms
/// prints the CPU it consumed WHILE watching (startup excluded), then closes
/// the watcher so the loop is free to exit.
const SOURCE: &str = r#"
import fs from "node:fs";
const dir = process.argv[2];
const windowMs = Number(process.argv[3]);
const watcher = fs.watch(dir, { recursive: true }, (eventType: string, filename: any) => {
  console.log("EVENT:" + eventType + ":" + String(filename).replace(/\\/g, "/"));
});
const start = process.cpuUsage();
console.log("READY");
setTimeout(() => {
  const used = process.cpuUsage(start);
  console.log("CPU_MS:" + ((used.user + used.system) / 1000).toFixed(1));
  watcher.close();
  console.log("CLOSED");
}, windowMs);
"#;

fn compile(dir: &Path, source: &str) -> PathBuf {
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
    output
}

fn build_tree(root: &Path) {
    for d in 0..DIR_COUNT {
        let dir = root.join(format!("sub{d:02}"));
        std::fs::create_dir_all(&dir).expect("create subdir");
        for f in 0..(FILE_COUNT / DIR_COUNT) {
            std::fs::write(dir.join(format!("f{f:03}.txt")), "x").expect("write file");
        }
    }
}

struct Fixture {
    child: Child,
    lines: Receiver<(String, Instant)>,
}

fn spawn_fixture(bin: &Path, tree: &Path, force_poll: bool) -> Fixture {
    let mut command = Command::new(bin);
    command
        .arg(tree)
        .arg(WINDOW_MS.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    if force_poll {
        command.env("PERRY_FS_WATCH_POLL", "1");
    } else {
        command.env_remove("PERRY_FS_WATCH_POLL");
    }
    let mut child = command.spawn().expect("spawn compiled binary");
    let stdout = child.stdout.take().expect("piped stdout");
    let (tx, rx) = mpsc::channel::<(String, Instant)>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(l) => {
                    if tx.send((l, Instant::now())).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    Fixture { child, lines: rx }
}

impl Fixture {
    fn wait_line(
        &self,
        predicate: impl Fn(&str) -> bool,
        timeout: Duration,
        what: &str,
    ) -> (String, Instant) {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let (line, at) = self
                .lines
                .recv_timeout(remaining)
                .unwrap_or_else(|_| panic!("no {what} line within {timeout:?}"));
            if predicate(&line) {
                return (line, at);
            }
        }
    }
}

struct Outcome {
    cpu_ms: f64,
    detect_latency: Duration,
}

fn run_watch_window(label: &str, force_poll: bool) -> Outcome {
    let dir = std::env::temp_dir().join(format!("perry_9591_{label}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let tree = dir.join("tree");
    build_tree(&tree);
    let bin = compile(&dir, SOURCE);

    let mut fixture = spawn_fixture(&bin, &tree, force_poll);
    fixture.wait_line(|l| l == "READY", Duration::from_secs(30), "READY");

    // Let the loop settle, then make one change and time its arrival.
    std::thread::sleep(Duration::from_millis(500));
    let probe = tree.join("sub05").join("probe_new_file.txt");
    let touched = Instant::now();
    std::fs::write(&probe, "hello").expect("write probe file");
    let (_, seen_at) = fixture.wait_line(
        |l| l.starts_with("EVENT:") && l.ends_with("sub05/probe_new_file.txt"),
        Duration::from_secs(10),
        "EVENT for the probe file",
    );
    let detect_latency = seen_at.duration_since(touched);

    let (cpu_line, _) = fixture.wait_line(
        |l| l.starts_with("CPU_MS:"),
        Duration::from_millis(WINDOW_MS + 15_000),
        "CPU_MS",
    );
    let cpu_ms: f64 = cpu_line["CPU_MS:".len()..].parse().expect("parse CPU_MS");
    fixture.wait_line(|l| l == "CLOSED", Duration::from_secs(10), "CLOSED");

    // With the watcher closed nothing keeps the loop alive: the process must
    // exit on its own (the old interval timer was ref'd; the new liveness
    // slot must release the loop the same way).
    let exit_deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = fixture.child.try_wait().expect("try_wait") {
            break status;
        }
        assert!(
            Instant::now() < exit_deadline,
            "{label}: the process did not exit after watcher.close() — a closed \
             watcher still reports itself active to the event loop"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success(), "{label}: fixture exited with {status}");
    let _ = std::fs::remove_dir_all(&dir);

    eprintln!("{label}: cpu {cpu_ms:.1} ms over {WINDOW_MS} ms window, change seen after {detect_latency:?}");
    Outcome {
        cpu_ms,
        detect_latency,
    }
}

#[test]
fn native_watch_of_three_thousand_files_is_idle_and_prompt() {
    let outcome = run_watch_window("native", false);
    // 5 % of the window. The pre-fix walker lands around 1 600 ms here.
    let budget_ms = WINDOW_MS as f64 * 0.05;
    assert!(
        outcome.cpu_ms < budget_ms,
        "fs.watch burned {:.1} ms of CPU over a {WINDOW_MS} ms window watching {FILE_COUNT} files \
         (budget {budget_ms:.0} ms) — the watcher is walking the tree on a timer again",
        outcome.cpu_ms
    );
    assert!(
        outcome.detect_latency < Duration::from_secs(1),
        "a new file took {:?} to surface — OS events deliver in milliseconds",
        outcome.detect_latency
    );
}

#[test]
fn poll_fallback_paces_itself_to_the_budget() {
    let outcome = run_watch_window("poll", true);
    // The walker sleeps 20 × its own duration between passes, so its duty
    // cycle is ≤ 5 % plus the granularity of one walk. Allow 12.5 % so a
    // loaded CI box cannot flake it; the pre-fix 25 ms cadence is 41 %.
    let budget_ms = WINDOW_MS as f64 * 0.125;
    assert!(
        outcome.cpu_ms < budget_ms,
        "the poll fallback burned {:.1} ms of CPU over a {WINDOW_MS} ms window watching {FILE_COUNT} \
         files (budget {budget_ms:.0} ms) — its interval is not scaling with the walk",
        outcome.cpu_ms
    );
    // ~10 ms walk ⇒ ~200 ms interval; well inside 3 s even on a slow disk.
    assert!(
        outcome.detect_latency < Duration::from_secs(3),
        "the poll fallback took {:?} to see a new file",
        outcome.detect_latency
    );
}
