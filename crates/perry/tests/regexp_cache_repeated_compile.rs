//! Regression: `js_regexp_new` must not recompile a pattern it has already
//! compiled+cached just to re-validate it.
//!
//! `js_regexp_new` validated each pattern by compiling it with both the `regex`
//! and `fancy-regex` engines — BEFORE consulting REGEX_CACHE. So a regex
//! literal evaluated in a hot loop (e.g. string-width's `emojiRegex()`, which
//! returns a fresh `/…/g` each call) recompiled its automaton on every call
//! even though the compiled form was already cached. For the 12807-char
//! emoji-regex that was ~99% CPU for minutes during ink's flexbox layout.
//!
//! Fix: skip the expensive validation compile when `(pattern, flags)` is
//! already in REGEX_CACHE (a cached pattern is by definition compilable). This
//! test guards CORRECTNESS of that skip — the cached regex must keep matching
//! correctly, each `RegExp` object must keep its own `lastIndex`, and invalid
//! patterns (never cached) must still throw `SyntaxError`.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    // Run under a wall-clock timeout so a regression to per-call recompilation
    // (pathologically slow for large patterns) fails fast instead of stalling.
    let mut child = Command::new(&output)
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn compiled binary");
    let timeout = Duration::from_secs(30);
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("try_wait on compiled binary") {
            break status;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            panic!("compiled binary did not exit within {timeout:?} — regex compile-cache likely regressed");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut stdout = String::new();
    if let Some(mut out) = child.stdout.take() {
        out.read_to_string(&mut stdout).ok();
    }
    let mut stderr = String::new();
    if let Some(mut err) = child.stderr.take() {
        err.read_to_string(&mut stderr).ok();
    }
    assert!(
        status.success(),
        "compiled binary failed\nstatus: {status:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    stdout
}

#[test]
fn repeated_regex_literal_stays_correct_and_independent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = compile_and_run(
        dir.path(),
        r#"
// A fresh regex literal each iteration → first compiles, rest hit the cache.
// The cached automaton must keep producing correct matches.
let ok = 0;
for (let i = 0; i < 1000; i++) {
  const re = /(\d+)-(\w+)/;
  const m = re.exec("42-foo");
  if (m && m[1] === "42" && m[2] === "foo") ok++;
}
console.log("matches:" + ok);

// Two RegExp objects built from the same pattern share the compiled automaton
// (cache) but must keep INDEPENDENT lastIndex (fresh header per construction).
const a = /x/g;
const b = /x/g;
a.exec("xx");                       // advances a.lastIndex past the first match
console.log("a-advanced:" + (a.lastIndex > 0));
console.log("b-independent:" + (b.lastIndex === 0));

// Flags participate in the cache key: /x/ vs /x/g vs /x/i are distinct.
console.log("flags-g:" + (/x/g.flags));
console.log("flags-i:" + (/x/i.flags));

// Invalid patterns are never cached, so they must still throw on every call.
let threw = 0;
for (let i = 0; i < 3; i++) {
  try { new RegExp("("); } catch (e) { if (e instanceof SyntaxError) threw++; }
}
console.log("invalid-throws:" + threw);
"#,
    );

    assert!(
        out.contains("matches:1000"),
        "cached regex stopped matching: {out}"
    );
    assert!(
        out.contains("a-advanced:true"),
        "lastIndex not advancing: {out}"
    );
    assert!(
        out.contains("b-independent:true"),
        "cached regexes wrongly share lastIndex: {out}"
    );
    assert!(out.contains("flags-g:g"), "g-flag regex wrong: {out}");
    assert!(out.contains("flags-i:i"), "i-flag regex wrong: {out}");
    assert!(
        out.contains("invalid-throws:3"),
        "invalid pattern stopped throwing: {out}"
    );
}
