//! A class's standalone constructor (`<prefix>__<class>_constructor`, the
//! body `new` of a class value and `super(...)` call) reads the facts of
//! the constructor's own `this`: the store `this.k = v` into a definite
//! birth field of a prefix-stable class is a slot store, as it is in the
//! inlined `new Packet(...)`, not the by-name `js_put_value_set` site.
//!
//! The standalone body used to be compiled as a function with id 0, so its
//! `this` read the facts of whichever function has HIR id 0 (here `first`).

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn clean(cmd: &mut Command) -> &mut Command {
    for (key, _) in std::env::vars() {
        if key.starts_with("PERRY_") || key == "NODE_OPTIONS" {
            cmd.env_remove(key);
        }
    }
    cmd
}

/// Compile `source`; return the saved LLVM IR of the entry module and the
/// program's stdout.
fn compile_and_run(dir: &Path, source: &str) -> (String, String) {
    let src = dir.join("main.ts");
    std::fs::write(&src, source).unwrap();
    let exe = dir.join("main_bin");
    let ll_dir = dir.join("ll");
    std::fs::create_dir_all(&ll_dir).unwrap();
    let mut cmd = Command::new(perry_bin());
    let out = clean(&mut cmd)
        .current_dir(dir)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_SAVE_LL", &ll_dir)
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("compile");
    assert!(
        out.status.success(),
        "compile failed\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ll = std::fs::read_to_string(ll_dir.join("main_ts.ll")).expect("saved main_ts.ll");
    let mut run = Command::new(&exe);
    let ran = clean(&mut run).output().expect("run");
    assert!(ran.status.success(), "run failed: {:?}", ran.status);
    (ll, String::from_utf8_lossy(&ran.stdout).into_owned())
}

/// The body of the function defined as `@<symbol>(`.
fn body<'a>(ir: &'a str, symbol: &str) -> &'a str {
    let head = format!("@{symbol}(");
    let at = ir
        .match_indices("define ")
        .map(|(i, _)| i)
        .find(|&i| {
            ir[i..]
                .lines()
                .next()
                .is_some_and(|line| line.contains(&head))
        })
        .unwrap_or_else(|| panic!("no function {symbol}"));
    let end = ir[at..].find("\n}\n").map_or(ir.len(), |e| at + e);
    &ir[at..end]
}

const PACKETS: &str = r#"
function first() { return 1; }
class Packet {
  constructor(link, kind) { this.link = link; this.kind = kind; this.count = 0; }
  bump() { this.count = this.count + 1; return this.count; }
}
const make = [() => new Packet(null, 1)][0];
let s = 0;
for (let i = 0; i < 1000; i++) { const p = new Packet(null, i); s += p.kind + p.bump(); }
console.log(s + first() + make().kind);
"#;

#[test]
fn standalone_constructor_stores_this_fields_through_its_own_facts() {
    let dir = tempfile::tempdir().unwrap();
    let (ir, out) = compile_and_run(dir.path(), PACKETS);
    assert_eq!(out.trim(), "500502");
    let ctor = body(&ir, "main_ts__Packet_constructor");
    assert!(
        !ctor.contains("@js_put_value_set("),
        "the standalone constructor's `this.k = v` stores are by-name sites:\n{ctor}"
    );
}
