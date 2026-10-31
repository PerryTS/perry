//! A method called on `this` from a constructor can read a key the
//! constructor assigns later: it reads `undefined`. Facts must not claim the
//! key is already set (`this.p === undefined` folded to false).

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

fn run(whole: bool) -> String {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("main.js");
    std::fs::write(
        &src,
        r#"class P { constructor() { this.v = 5; } }
class A {
  constructor(early) { this.s = early ? this.peek() : "-"; this.p = new P(); }
  peek() { return "t=" + typeof this.p + " " + (this.p === undefined); }
}
console.log(new A(false).s, new A(true).s);
"#,
    )
    .unwrap();
    let exe = dir.path().join("main");
    let mut cmd = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_perry")));
    clean(&mut cmd).current_dir(dir.path()).env("PERRY_NO_CACHE", "1");
    if whole {
        cmd.env("PERRY_FACTS_UNITS", "0");
    }
    let out = cmd.arg("compile").arg(&src).arg("-o").arg(&exe).output().expect("compile");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let mut run = Command::new(&exe);
    let ran = clean(&mut run).output().expect("run");
    assert!(ran.status.success(), "run failed: {:?}", ran.status);
    String::from_utf8_lossy(&ran.stdout).trim().to_string()
}

#[test]
fn a_method_called_from_the_constructor_sees_the_unassigned_key() {
    // node: `- t=undefined true`
    assert_eq!(run(false), "- t=undefined true", "per unit");
    assert_eq!(run(true), "- t=undefined true", "whole program");
}
