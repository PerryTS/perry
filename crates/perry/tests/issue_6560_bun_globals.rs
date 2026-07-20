//! #6560 — Bun globals shim pack (Tier 0 of Bun-app support; driver:
//! opencode). Covers the full issue surface end-to-end on compiled binaries:
//!
//! - `Bun.stringWidth` (ANSI stripping, East-Asian width, emoji/ZWJ
//!   clustering, `countAnsiEscapeCodes` / `ambiguousIsNarrow` options)
//! - `Bun.hash` (wyhash64, BigInt result, number + bigint seeds)
//! - `Bun.file` (`.text()` / `.json()` / `.arrayBuffer()` / `.exists()` /
//!   `.size` / `.type`, ENOENT rejection shape)
//! - `Bun.write` (path / BunFile / `Bun.stdout` destinations, string /
//!   Uint8Array / ArrayBuffer / BunFile payloads, parent-dir creation,
//!   byte-count result)
//! - `Bun.stdin.text()` (read-all of piped stdin)
//! - `import { pathToFileURL, fileURLToPath } from "bun"` (node:url
//!   semantics) + namespace-import form
//! - guards: `typeof Bun` stays `"undefined"` (node bundles feature-detect
//!   Bun this way), and a user binding named `Bun` shadows the global.
//!
//! Every expected value below was produced by real Bun v1.3.12.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

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

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = compile(dir.path(), source);
    // Run from the tempdir: the fixtures create files at `process.cwd()`.
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
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn bun_string_width_matches_bun() {
    let stdout = compile_and_run(
        r#"
const w = (s: string, o?: any) => Bun.stringWidth(s, o);
console.log(w(""), w("a"), w("hello"), w("héllo"));
console.log(w("\x1b[31mred\x1b[39m"), w("\x1b[31mred\x1b[39m", { countAnsiEscapeCodes: true }));
console.log(w("你好"), w("ｈｅｌｌｏ"), w("ﾊﾛｰ"));
console.log(w("\u{1f44d}"), w("\u{1f469}\u{200d}\u{1f4bb}"), w("\u{1f469}\u{200d}\u{1f469}\u{200d}\u{1f466}\u{200d}\u{1f466}"), w("\u{1f44b}\u{1f3fd}"));
console.log(w("é"), w("​"), w("‍"));
console.log(w("①"), w("①", { ambiguousIsNarrow: false }), w("α", { ambiguousIsNarrow: false }));
console.log(w("\t"), w("a\tb"), w("\u{1f1e9}\u{1f1ea}"), w("☀️"), w("☀"));
console.log(w("\x1b]8;;http://x\x1b\\text\x1b]8;;\x1b\\"), w("\x1b[31"), w("\x1bM"));
console.log(Bun.stringWidth(123 as any));
"#,
    );
    let expected = "\
0 1 5 5
3 11
4 10 3
2 2 2 2
1 0 0
1 2 2
0 2 2 2 1
4 0 1
3
";
    assert_eq!(stdout, expected);
}

#[test]
fn bun_hash_is_wyhash_bigint() {
    let stdout = compile_and_run(
        r#"
console.log(typeof Bun.hash("abc"));
console.log(Bun.hash("").toString(16));
console.log(Bun.hash("abc").toString(16));
console.log(Bun.hash("hello world").toString(16));
console.log(Bun.hash("skill-discovery-cache-key").toString(16));
console.log(Bun.hash("héllo wörld ünïcödé").toString(16));
console.log(Bun.hash("abc", 42).toString(16));
console.log(Bun.hash("abc", 42n).toString(16));
console.log(Bun.hash(new Uint8Array([1, 2, 3])).toString(16));
"#,
    );
    let expected = "\
bigint
409638ee2bde459
2a4f1d7cb516c72
668d5e431c3b2573
f51cb10d1f69d049
5b4ab9e3ff3751ee
729d41f062dc5b37
729d41f062dc5b37
c3e927b407f2b4b3
";
    assert_eq!(stdout, expected);
}

#[test]
fn bun_file_write_roundtrip() {
    let stdout = compile_and_run(
        r#"
async function main() {
  const dir = process.cwd();
  const p = dir + "/bf-test.txt";

  const n = await Bun.write(p, "hello wörld");
  console.log("write:", n);

  const f = Bun.file(p);
  console.log("size:", f.size, "type:", f.type);
  console.log("text:", JSON.stringify(await f.text()));
  const ab = await f.arrayBuffer();
  console.log("ab:", ab.byteLength);
  console.log("exists:", await f.exists());

  const missing = Bun.file(dir + "/does-not-exist.txt");
  console.log("missing size:", missing.size, "exists:", await missing.exists());
  try {
    await missing.text();
    console.log("missing text: no throw");
  } catch (e: any) {
    console.log("missing code:", e.code);
  }

  await Bun.write(p, '{"a": 1}');
  const parsed = await Bun.file(p).json();
  console.log("json:", JSON.stringify(parsed));

  console.log("write u8:", await Bun.write(p, new Uint8Array([104, 105])));
  console.log("text2:", JSON.stringify(await Bun.file(p).text()));
  console.log("write ab:", await Bun.write(p, new Uint8Array([104, 105]).buffer as ArrayBuffer));

  const p2 = dir + "/bf-copy.txt";
  console.log("write BunFile:", await Bun.write(p2, Bun.file(p)), JSON.stringify(await Bun.file(p2).text()));
  console.log("write dest BunFile:", await Bun.write(Bun.file(p2), "via handle"), JSON.stringify(await Bun.file(p2).text()));

  console.log("nested:", await Bun.write(dir + "/nested/deep/x.txt", "n"));

  const sn = await Bun.write(Bun.stdout, "TO-STDOUT\n");
  console.log("stdout ret:", sn);
}
main();
"#,
    );
    let expected = "\
write: 12
size: 12 type: text/plain;charset=utf-8
text: \"hello wörld\"
ab: 12
exists: true
missing size: 0 exists: false
missing code: ENOENT
json: {\"a\":1}
write u8: 2
text2: \"hi\"
write ab: 2
write BunFile: 2 \"hi\"
write dest BunFile: 10 \"via handle\"
nested: 1
TO-STDOUT
stdout ret: 10
";
    assert_eq!(stdout, expected);
}

#[test]
fn bun_stdin_text_reads_all() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = compile(
        dir.path(),
        r#"
async function main() {
  const t = await Bun.stdin.text();
  console.log(JSON.stringify(t));
  console.log("stdin size:", Bun.stdin.size);
}
main();
"#,
    );
    let mut child = Command::new(&output)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn compiled binary");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all("piped ünicode input\nline2".as_bytes())
        .expect("write stdin");
    let run = child.wait_with_output().expect("wait");
    assert!(
        run.status.success(),
        "compiled binary failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "\"piped ünicode input\\nline2\"\nstdin size: Infinity\n"
    );
}

#[test]
fn bun_module_url_aliases() {
    let stdout = compile_and_run(
        r#"
import { pathToFileURL, fileURLToPath } from "bun";
import * as bun from "bun";
console.log(pathToFileURL("/tmp/a b.txt").href);
console.log(fileURLToPath("file:///tmp/a%20b.txt"));
console.log(bun.pathToFileURL("/tmp/x y.txt").href);
console.log(bun.fileURLToPath("file:///tmp/x%20y.txt"));
"#,
    );
    let expected = "\
file:///tmp/a%20b.txt
/tmp/a b.txt
file:///tmp/x%20y.txt
/tmp/x y.txt
";
    assert_eq!(stdout, expected);
}

/// Node-targeting bundles feature-detect Bun via `typeof Bun`; the shim must
/// NOT make that report anything but "undefined" (claude-code and friends
/// would otherwise flip onto their Bun code paths). A user binding named
/// `Bun` must also shadow the global surface.
#[test]
fn bun_feature_detection_and_shadowing_unaffected() {
    let stdout = compile_and_run(
        r#"
console.log(typeof Bun);
function scoped() {
  const Bun = { stringWidth: (_s: string) => 999 };
  return Bun.stringWidth("hello");
}
console.log(scoped());
"#,
    );
    assert_eq!(stdout, "undefined\n999\n");
}
