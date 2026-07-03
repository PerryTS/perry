//! Regression test for #5920: spawned async tasks starved after a recompile
//! with fresh auto-optimize archives.
//!
//! The archives-fresh fast path links the well-known wrapper archives BEFORE
//! `libperry_stdlib.a` (`prefer_well_known_before_stdlib`), so the wrapper's
//! bundled `perry_runtime` codegen unit used to become the first-definition
//! winner for every extern runtime symbol — while perry-stdlib's own code kept
//! using ITS bundled runtime copy through LTO-promoted `.llvm.` internals. Two
//! copies of the event pump's mutable state: `spawn()` registered the
//! wait-driver in stdlib's copy, `js_wait_for_event` read the wrapper's
//! never-written copy, parked on the condvar fallback, and every spawned task
//! starved. The FIRST compile of the same source (full auto-optimize rebuild,
//! wrappers after stdlib) was unaffected — the bug only appeared on
//! recompiles, once the auto-opt archives were warm.
//!
//! The program below exercises exactly the starving shape: a fire-and-forget
//! `fetch().then(...)` (no top-level await, so completion depends on the
//! event loop driving the spawned task via the wait-driver, not on a
//! `block_on`) against an in-process `node:http` server (which pulls the
//! perry-ext-http wrapper into the link). A 250 ms interval watchdog turns a
//! starved fetch into a deterministic `FAIL` exit within 10 s. Compiling
//! TWICE and running BOTH binaries covers both link shapes regardless of
//! whether a previous test already warmed the archives.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const SOURCE: &str = r#"
const http = require('http');

const server = http.createServer((_req: any, res: any) => {
  res.end('pong');
});

server.listen(0, () => {
  const port = (server as any).address().port;
  let done = false;

  // Fire-and-forget: the resolution depends on the main loop driving the
  // spawned task — the exact path that starved pre-fix (#5920).
  fetch('http://127.0.0.1:' + port + '/')
    .then((r: any) => r.text())
    .then((body: string) => {
      done = true;
      console.log('FETCH-DONE', body);
    })
    .catch((e: any) => {
      console.log('FETCH-ERR', String(e));
      process.exit(1);
    });

  let ticks = 0;
  const iv = setInterval(() => {
    ticks++;
    if (done) {
      clearInterval(iv);
      server.close();
      console.log('PASS');
      process.exit(0);
    }
    if (ticks >= 40) {
      clearInterval(iv);
      server.close();
      console.log('FAIL: fetch starved');
      process.exit(1);
    }
  }, 250);
});
"#;

fn compile(dir: &std::path::Path, entry: &std::path::Path, output: &std::path::Path) -> String {
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(entry)
        .arg("-o")
        .arg(output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    String::from_utf8_lossy(&compile.stderr).into_owned()
}

fn run(dir: &std::path::Path, output: &std::path::Path, shape: &str) {
    let run = Command::new(output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success() && stdout.contains("PASS"),
        "{shape}: spawned fetch starved (#5920)\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        stdout,
        String::from_utf8_lossy(&run.stderr)
    );
}

/// Compile the identical source twice and run both binaries. Pre-fix, the
/// second compile (auto-opt archives fresh → wrappers linked before stdlib)
/// produced a binary whose fire-and-forget fetch never resolved.
#[test]
fn fire_and_forget_fetch_survives_recompile() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(&entry, SOURCE).expect("write entry");

    let first = dir.path().join("main_first");
    compile(dir.path(), &entry, &first);
    run(dir.path(), &first, "first compile");

    let second = dir.path().join("main_second");
    let _ = compile(dir.path(), &entry, &second);
    // The behavioral gate IS the regression signal: pre-fix, this second
    // binary (archives-fresh → wrappers-before-stdlib link) ticked to the
    // watchdog FAIL with the fetch never resolving. No assertion on the
    // strip-dedup log wording — the exact messages are not a contract.
    run(
        dir.path(),
        &second,
        "second compile (archives-fresh link shape)",
    );
}
