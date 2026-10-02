//! Step 5C ConstFn lanes are on by default for executables. An unset
//! `PERRY_CONSTFN_SHAPE` and `=1` both build the ConstFn method lane; only an
//! explicit `=0` turns it off. The program has no class, so it also covers the
//! classless-module path through frontend shape discovery, which used to skip
//! such modules unless the knob was `1`.
//!
//! The observable is the runtime's own method-site report: a site primed from
//! a ConstFn lane counts in `primes_constfn`. Every mode must print the same
//! result.

use std::path::PathBuf;
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

const PROGRAM: &str = r#"
const counter = {
  base: 3,
  step(x: number): number {
    return x + this.base;
  },
};
function make(base: number) {
  return { base, step: (x: number): number => x + base };
}
function drive(o: any, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s = o.step(s);
  return s;
}
console.log(drive(counter, 1000), drive(make(5), 1000));
"#;

/// Compile `PROGRAM` with the knob as given (`None` = unset), run it with
/// the method-site report on, and return (stdout, primes_constfn).
fn compile_and_run(setting: Option<&str>) -> (String, u64) {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("main.ts");
    std::fs::write(&src, PROGRAM).unwrap();
    let exe = dir.path().join("main_bin");
    let mut compile = Command::new(perry_bin());
    clean(&mut compile)
        .current_dir(dir.path())
        .env("PERRY_NO_CACHE", "1");
    if let Some(value) = setting {
        compile.env("PERRY_CONSTFN_SHAPE", value);
    }
    let compiled = compile
        .arg("compile")
        .arg(&src)
        .arg("--no-auto-optimize")
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("compile");
    assert!(
        compiled.status.success(),
        "compile failed (PERRY_CONSTFN_SHAPE={setting:?})\n{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let mut run = Command::new(&exe);
    let ran = clean(&mut run)
        .current_dir(dir.path())
        .env("PERRY_METHOD_SITE_STATS", "1")
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&ran.stderr).into_owned();
    assert!(
        ran.status.success(),
        "run failed (PERRY_CONSTFN_SHAPE={setting:?}): {:?}\n{stderr}",
        ran.status
    );
    let report = stderr
        .lines()
        .find(|line| line.starts_with("[method-site]"))
        .unwrap_or_else(|| panic!("no method-site report ({setting:?}):\n{stderr}"));
    let primes = report
        .split_whitespace()
        .find_map(|field| field.strip_prefix("primes_constfn="))
        .unwrap_or_else(|| panic!("no primes_constfn in {report:?}"))
        .parse()
        .expect("primes_constfn is a count");
    (String::from_utf8(ran.stdout).unwrap(), primes)
}

#[test]
fn constfn_lane_is_on_unless_explicitly_disabled() {
    let (unset_out, unset_primes) = compile_and_run(None);
    let (on_out, on_primes) = compile_and_run(Some("1"));
    let (off_out, off_primes) = compile_and_run(Some("0"));
    assert_eq!(unset_out, "3000 5000\n");
    assert_eq!(on_out, unset_out);
    assert_eq!(off_out, unset_out);
    assert!(unset_primes > 0, "unset must build the ConstFn lane");
    assert!(on_primes > 0, "=1 must build the ConstFn lane");
    assert_eq!(off_primes, 0, "=0 must not build the ConstFn lane");
}
