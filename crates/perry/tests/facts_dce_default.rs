//! Dead-body removal (`PERRY_FACTS_DCE`) is on by default. An unset switch
//! and `=1` both emit a body the whole-program reach calls inactive as an
//! `llvm.trap` entry; only an explicit `=0` keeps the body. Every setting
//! prints the same output.

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
function neverCalled() { return "never"; }
function alsoDead(x: number) { return neverCalled() + x; }
class Unused { m() { return 1; } get g() { return 2; } }
const deadArrow = () => alsoDead(1);
if (process.argv.length < 0) { console.log(deadArrow(), new Unused().m()); }
console.log("live", [1, 2, 3].map((x) => x * 2).join(","));
"#;

/// Compile `PROGRAM` with the switch as given (`None` = unset), run it, and
/// return (stdout, number of `llvm.trap` calls in the saved IR).
fn compile_and_run(setting: Option<&str>) -> (String, usize) {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("main.ts");
    std::fs::write(&src, PROGRAM).unwrap();
    let exe = dir.path().join("main_bin");
    let ll_dir = dir.path().join("ll");
    std::fs::create_dir_all(&ll_dir).unwrap();
    let mut compile = Command::new(perry_bin());
    clean(&mut compile)
        .current_dir(dir.path())
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_FACTS_UNITS", "0")
        .env("PERRY_SAVE_LL", &ll_dir);
    if let Some(value) = setting {
        compile.env("PERRY_FACTS_DCE", value);
    }
    let compiled = compile
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("compile");
    assert!(
        compiled.status.success(),
        "compile failed (PERRY_FACTS_DCE={setting:?})\n{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let traps = std::fs::read_dir(&ll_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "ll"))
        .map(|e| {
            std::fs::read_to_string(e.path())
                .unwrap()
                .matches("call void @llvm.trap()")
                .count()
        })
        .sum();
    let ran = Command::new(&exe).output().expect("run");
    assert!(
        ran.status.success(),
        "run failed (PERRY_FACTS_DCE={setting:?})"
    );
    (String::from_utf8_lossy(&ran.stdout).into_owned(), traps)
}

#[test]
fn dead_body_removal_is_on_by_default_and_zero_turns_it_off() {
    let (unset, unset_traps) = compile_and_run(None);
    let (one, one_traps) = compile_and_run(Some("1"));
    let (zero, zero_traps) = compile_and_run(Some("0"));
    assert_eq!(unset, "live 2,4,6\n");
    assert_eq!(one, unset);
    assert_eq!(zero, unset);
    assert!(
        unset_traps >= 3,
        "default removes the dead bodies: {unset_traps}"
    );
    assert_eq!(one_traps, unset_traps, "=1 is the default spelled out");
    assert_eq!(zero_traps, 0, "=0 keeps every body");
}
