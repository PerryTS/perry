//! Regression test for #6067: an out-of-bounds index of a string LOCAL inside a
//! generator/async function returned `""` instead of `undefined`.
//!
//! `s[i]` (string indexing) must yield `undefined` for a non-canonical or
//! out-of-bounds index — unlike `s.charAt(i)`, which yields `""`. Statically
//! string-typed receivers took the correct static path (`js_string_index_get`).
//! But a generator/async body is CPS-transformed and its locals are boxed for
//! cross-state persistence; that erases the local's static type, so `line[i]`
//! on a `const line = ""` (or any local string) is not seen as a string and
//! falls to the runtime tag dispatcher `js_dyn_index_get` — whose string arm
//! called `js_string_char_at` directly (charAt semantics: OOB → `""`), missing
//! the `idx >= len → undefined` guard.
//!
//! The user-visible bite: the `yaml` package's lexer detects end-of-line with
//! `line[n] === undefined` (where `line = this.getLine()` is a local); with it
//! returning `""`, `parseDocument`'s `switch (line[n])` never matched
//! `case undefined`, took a non-advancing `default`, and the `*lex` state
//! machine spun forever — hanging `yaml.parse()` (and a large esbuild-bundled
//! CLI app that parses YAML front-matter at module-init time) at 100% CPU.
//!
//! Fix: `js_dyn_index_get`'s string arm routes through `js_string_index_get`
//! (canonical-index validation + OOB → `undefined`), matching the static path.
//!
//! Array / typed-array / object indexing inside a generator must stay correct
//! (they already dispatch by runtime shape through the same helper). Expected
//! outputs are byte-for-byte what `node --experimental-strip-types` prints; the
//! lexer-shaped case is timeout-bounded so a regression FAILS (spins) instead of
//! hanging the test.

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
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
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let mut child = Command::new(&output)
        .current_dir(dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn compiled binary");

    let timeout = Duration::from_secs(30);
    let start = Instant::now();
    loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => {
                let out = child.wait_with_output().expect("wait_with_output");
                assert!(
                    status.success(),
                    "compiled binary failed (exit {:?})\nstdout:\n{}\nstderr:\n{}",
                    status.code(),
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
                return String::from_utf8_lossy(&out.stdout).into_owned();
            }
            None => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!(
                        "compiled binary did not finish within {:?} — a generator \
                         local string OOB index regressed (returns \"\" not undefined)",
                        timeout
                    );
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

/// The core bug: OOB index of a string LOCAL in a generator is `undefined`.
#[test]
fn generator_local_string_oob_index_is_undefined() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function drain<T>(g: Generator<number, T, unknown>): T {
  let r = g.next(); while (!r.done) r = g.next(); return r.value;
}
function* g(): Generator<number, string, unknown> {
  const line = "";              // string local, boxed across the yield
  yield 1;
  const d = line[0];            // OOB on empty string
  return "empty=" + (d === undefined) + " type=" + typeof d;
}
function* h(): Generator<number, string, unknown> {
  const s = "ab";
  yield 1;
  return "in=" + s[0] + s[1] + " oob=" + (s[5] === undefined);
}
console.log(drain(g()));
console.log(drain(h()));
"#,
    );
    assert_eq!(stdout, "empty=true type=undefined\nin=ab oob=true\n");
}

/// The async equivalent (async fns are CPS-transformed the same way).
#[test]
fn async_local_string_oob_index_is_undefined() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
async function f(): Promise<string> {
  const line = "".concat("");   // runtime-produced empty string local
  await Promise.resolve(0);
  const d = line[0];
  return "empty=" + (d === undefined) + " len=" + line.length;
}
f().then((v) => console.log(v));
"#,
    );
    assert_eq!(stdout, "empty=true len=0\n");
}

/// Array / typed-array / object indexing inside a generator must stay correct.
#[test]
fn generator_local_array_typedarray_object_index_unaffected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function drain<T>(g: Generator<number, T, unknown>): T {
  let r = g.next(); while (!r.done) r = g.next(); return r.value;
}
function* garr(): Generator<number, string, unknown> { const a = [10, 20, 30]; yield 1; return "arr:" + a[1] + "," + a[9]; }
function* gta(): Generator<number, string, unknown> { const t = new Uint8Array([5, 6, 7]); yield 1; return "ta:" + t[0] + "," + t[2]; }
function* gobj(): Generator<number, string, unknown> { const o: any = { 0: "zero" }; yield 1; return "obj:" + o[0] + "," + o[5]; }
console.log(drain(garr()));
console.log(drain(gta()));
console.log(drain(gobj()));
"#,
    );
    assert_eq!(stdout, "arr:20,undefined\nta:5,7\nobj:zero,undefined\n");
}

/// A `yaml`-lexer-shaped consumer: a generator scans a local string by index in
/// a loop whose only exit is an OOB read returning `undefined`. Pre-fix this
/// spun forever (`""` is truthy-distinct from `undefined`, so the loop never
/// hit its terminator).
#[test]
fn generator_string_scan_loop_terminates_on_oob() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function* scan(src: string): Generator<string, number, unknown> {
  const line = src.substring(0, src.length);   // local string
  let i = 0, n = 0;
  while (true) {
    const ch = line[i];                         // undefined past the end
    if (ch === undefined) break;
    yield ch;
    i++; n++;
  }
  return n;
}
const out: string[] = [];
const g = scan("ab:");
let r = g.next();
while (!r.done) { out.push(r.value as string); r = g.next(); }
console.log(out.join(",") + "|n=" + r.value);
"#,
    );
    assert_eq!(stdout, "a,b,:|n=3\n");
}
