//! Regression test: a closure declared FIRST in a single `let`/`const`
//! declaration that forward-references names bound by LATER declarators of the
//! SAME declaration must capture them (as boxed locals), not fall through to a
//! global-by-name lookup.
//!
//! The minified `new Promise` executor shape triggers this:
//! ```js
//! new Promise((resolve) => {
//!   let z = (w) => { clearTimeout(O); q.off(...); resolve(w); },
//!       Y = () => z(false),
//!       A = () => clearTimeout(O),
//!       O = setTimeout(z, K);   // z forward-refs O/Y/A in the same `let`
//!   ...
//! })
//! ```
//! Before the fix, `pre_register_forward_captured_lets` only recorded a
//! statement's closure-refs AFTER the whole declaration, so the later
//! declarators (`O`/`Y`/`A`) were never seen as forward-captured and lowered to
//! `js_global_get_or_throw_unresolved`. That globalization shifted the closure's
//! capture slots so its captured `resolve` read the wrong value — the promise
//! never settled and the awaiting caller hung forever.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile(dir: &std::path::Path, source: &str) -> PathBuf {
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

/// Run `bin`, failing if it doesn't exit within `secs` (the bug is a hang —
/// the promise never settles so `await` never returns).
fn run_with_timeout(bin: &std::path::Path, secs: u64) -> String {
    let mut child = Command::new(bin)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
        .expect("spawn compiled binary");
    let mut piped = child.stdout.take().expect("piped stdout");
    let reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = piped.read_to_string(&mut buf);
        buf
    });
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => {
                let stdout = reader.join().unwrap_or_default();
                assert!(
                    status.success(),
                    "binary exited non-zero: {status:?}\n{stdout}"
                );
                return stdout;
            }
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let stdout = reader.join().unwrap_or_default();
                panic!(
                    "regression: process hung >{secs}s — a closure's forward-ref to \
                     a later declarator in the same `let` globalized, so its captured \
                     `resolve` never settled the promise.\nstdout so far:\n{stdout}"
                );
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

#[test]
fn forward_captured_lets_in_comma_declaration_resolve_promise() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = compile(
        dir.path(),
        r#"
function makeP(): Promise<string> {
  return new Promise<string>((resolve) => {
    // z (declared FIRST) forward-references O/Y/A declared LATER in this
    // same `let`, and captures the executor param `resolve`.
    let z = (w: string) => { clearTimeout(O); off(Y); off(A); resolve(w); },
        Y = () => z("via-Y"),
        A = () => {},
        O: any = setTimeout(() => resolve("TIMEOUT"), 10000);
    function off(_f: any) {}
    setTimeout(Y, 5);   // fire Y -> z -> resolve("via-Y")
  });
}

async function main() {
  const r = await makeP();
  process.stdout.write("RESULT=" + r + "\n");
}
main();
"#,
    );
    let stdout = run_with_timeout(&bin, 20);
    assert_eq!(stdout, "RESULT=via-Y\n");
}
