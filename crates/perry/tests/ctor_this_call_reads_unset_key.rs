//! A key the constructor stores after code that runs with `this` (here a
//! method call on `this`) holds the hole until the store: the key is absent,
//! and a read continues to the prototype chain (object model §3.2). Code
//! that observes the instance then must see what node sees. The facts keep
//! such a key's slot but not its presence (`FieldFacts::reserved`), and an
//! elided read pairs its load with the hole compare. One program per
//! lowering that consumes the read; each runs per unit and whole program.

use std::path::PathBuf;
use std::process::Command;

fn clean(cmd: &mut Command) -> &mut Command {
    for (key, _) in std::env::vars() {
        if key.starts_with("PERRY_") || key == "NODE_OPTIONS" {
            cmd.env_remove(key);
        }
    }
    cmd
}

fn run(src_text: &str, whole: bool) -> String {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("main.js");
    std::fs::write(&src, src_text).unwrap();
    let exe = dir.path().join("main");
    let mut cmd = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_perry")));
    clean(&mut cmd)
        .current_dir(dir.path())
        .env("PERRY_NO_CACHE", "1");
    if whole {
        cmd.env("PERRY_FACTS_UNITS", "0");
    }
    let out = cmd
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("compile");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut run = Command::new(&exe);
    let ran = clean(&mut run).output().expect("run");
    assert!(
        ran.status.success(),
        "run failed: {:?}\n{}",
        ran.status,
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8_lossy(&ran.stdout).trim().to_string()
}

/// `node main.js` prints `expected`.
fn check(src: &str, expected: &str) {
    assert_eq!(run(src, false), expected, "per unit");
    assert_eq!(run(src, true), expected, "whole program");
}

#[test]
fn strict_equality_with_undefined() {
    check(
        r#"class P { constructor() { this.v = 5; } }
class A {
  constructor(early) { this.s = early ? this.peek() : "-"; this.p = new P(); }
  peek() { return "t=" + typeof this.p + " " + (this.p === undefined); }
}
console.log(new A(false).s, new A(true).s);
"#,
        "- t=undefined true",
    );
}

#[test]
fn loose_equality_with_null() {
    check(
        r#"class A {
  constructor() { this.s = this.peek(); this.p = { v: 1 }; }
  peek() { return [this.p == null, this.p != undefined, this.p === null].join(","); }
}
console.log(new A().s);
"#,
        "true,false,false",
    );
}

#[test]
fn nullish_coalescing() {
    check(
        r#"class A {
  constructor() { this.s = this.peek(); this.p = "set"; }
  peek() { return this.p ?? "dflt"; }
}
console.log(new A().s, new A().p);
"#,
        "dflt set",
    );
}

#[test]
fn string_conversion() {
    check(
        r#"class A {
  constructor() { this.s = this.peek(); this.p = 7; }
  peek() { return String(this.p) + "|" + this.p + "|" + `${this.p}`; }
}
console.log(new A().s);
"#,
        "undefined|undefined|undefined",
    );
}

#[test]
fn number_context() {
    check(
        r#"class A {
  constructor() { this.s = this.peek(); this.n = 3; }
  peek() { return this.n + 1; }
  later() { return this.n + 1; }
}
const a = new A();
console.log(a.s, a.later());
"#,
        "NaN 4",
    );
}

#[test]
fn absent_key_reads_the_prototype() {
    check(
        r#"class A {
  constructor() { this.s = this.peek(); this.p = "own"; }
  peek() { return this.p; }
}
A.prototype.p = "proto";
const a = new A();
console.log(a.s, a.p);
"#,
        "proto own",
    );
}

#[test]
fn call_through_the_absent_key_calls_the_prototype_method() {
    check(
        r#"class A {
  constructor() { this.r = this.run(); this.f = function () { return "own"; }; }
  run() { return this.f(); }
}
A.prototype.f = function () { return "proto"; };
const a = new A();
console.log(a.r, a.run());
"#,
        "proto own",
    );
}

#[test]
fn presence_and_enumeration() {
    check(
        r#"class A {
  constructor() { this.s = this.peek(); this.p = 1; this.q = 2; }
  peek() { return ["p" in this, Object.keys(this).join("+"), Object.prototype.hasOwnProperty.call(this, "p"), JSON.stringify(this), !this.p, typeof this.p].join(","); }
}
const a = new A();
console.log(a.s, Object.keys(a).join("+"));
"#,
        "false,,false,{},true,undefined s+p+q",
    );
}
