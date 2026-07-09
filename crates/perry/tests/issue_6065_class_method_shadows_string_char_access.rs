//! Regression test for #6065: a user class method whose name shadows a
//! `String.prototype` char-access method (`charAt` / `charCodeAt` /
//! `codePointAt`) must call the USER method, not the String builtin.
//!
//! The static string-method fast path was guarded by an arity heuristic
//! (`string_only_method_arity_ok`) so that a user method sharing a String
//! builtin name (e.g. joi's `internals.trim(value, schema)`) falls through to
//! runtime dispatch when the arg count can't match the builtin. But the
//! char-access methods ignore surplus args per spec, so that gate returns
//! `true` for ANY arg count — a user `charAt(n)` on a class instance was
//! therefore lowered to `String.prototype.charAt` with the receiver coerced to
//! `"[object Object]"`, so `this.charAt(0)` returned `"["`, `this.charAt(1)`
//! returned `"o"`, and so on.
//!
//! The `yaml` package's `Lexer.charAt(n) { return this.buffer[this.pos + n]; }`
//! is exactly this shape: with it mis-dispatched the tokenizer reads garbage
//! and its `*lex` state machine (`while (next) next = yield* this.parseNext(next)`)
//! never advances `pos`, spinning forever — hanging YAML parsing (and any large
//! esbuild-bundled CLI app that parses YAML at module-init time) at 100% CPU.
//!
//! Fix: don't take the static String path when the receiver's statically-known
//! class defines its own method, getter, or instance field of that name.
//!
//! Expected outputs are byte-for-byte what `node --experimental-strip-types`
//! prints. The generator/lexer cases are timeout-bounded so a regression FAILS
//! (spins) instead of hanging the test process.

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
                        "compiled binary did not finish within {:?} — a user \
                         `charAt`/`charCodeAt`/`codePointAt` method was mis-lowered \
                         to the String builtin (regression)",
                        timeout
                    );
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

/// A plain class method named `charAt` must call the user's method — not
/// `String.prototype.charAt` on a `"[object Object]"`-coerced receiver.
#[test]
fn plain_method_named_char_at_is_user_method() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
class Buf {
  buffer = "hello";
  pos = 0;
  charAt(n: number) { return this.buffer[this.pos + n]; }
}
const b = new Buf();
console.log(b.charAt(0) + b.charAt(1) + b.charAt(4));
"#,
    );
    assert_eq!(stdout, "heo\n");
}

/// A field holding a function value (`charAt = (n) => …`) shadows the builtin
/// the same way a method does — the receiver-class guard must treat instance
/// fields as defining the name, not just methods/getters.
#[test]
fn arrow_function_field_named_char_at_is_user_function() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
class Buf {
  buffer = "hello";
  pos = 0;
  charAt = (n: number) => this.buffer[this.pos + n];
}
const b = new Buf();
console.log(b.charAt(0) + b.charAt(1) + b.charAt(4));
"#,
    );
    assert_eq!(stdout, "heo\n");
}

/// `charCodeAt` and `codePointAt` are affected by the same arity-gate no-op.
#[test]
fn plain_methods_named_char_code_at_and_code_point_at_are_user_methods() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
class C {
  data = [10, 20, 30];
  charCodeAt(i: number) { return this.data[i] + 1; }
  codePointAt(i: number) { return this.data[i] * 2; }
}
const c = new C();
console.log(c.charCodeAt(0) + "," + c.codePointAt(2));
"#,
    );
    assert_eq!(stdout, "11,60\n");
}

/// A genuine string receiver must still get `String.prototype.charAt` — the
/// fix must not break real string char access.
#[test]
fn real_string_char_at_still_works() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const s = "world";
console.log(s.charAt(0) + s.charAt(4) + "|" + s.charCodeAt(1));
"#,
    );
    assert_eq!(stdout, "wd|111\n");
}

/// The exact hang shape: a generator class method using its own `charAt` in a
/// yielding `switch` loop that only terminates when `charAt` returns the real
/// chars. Mirrors the `yaml` `Lexer` indicator scan.
#[test]
fn generator_using_own_char_at_in_switch_loop_terminates() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
class Lexer {
  buffer = "";
  pos = 0;
  charAt(n: number) { return this.buffer[this.pos + n]; }
  *lex(): Generator<string, number, unknown> {
    let count = 0;
    loop: while (true) {
      switch (this.charAt(0)) {
        case 'a':
          yield 'A'; this.pos++; count++; continue loop;
        case ':':
          yield 'C'; this.pos++; count++; continue loop;
      }
      break loop;
    }
    return count;
  }
}
const lx = new Lexer();
lx.buffer = "aa:aZ";
const out: string[] = [];
const g = lx.lex();
let r = g.next();
while (!r.done) { out.push(r.value as string); r = g.next(); }
console.log(out.join(",") + "|count=" + r.value + "|pos=" + lx.pos);
"#,
    );
    assert_eq!(stdout, "A,A,C,A|count=4|pos=4\n");
}
