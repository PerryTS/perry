//! #9599 — opt-in Bun platform mode with a real global namespace.

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile(dir: &Path, platform: Option<&str>) -> PathBuf {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    let mut command = Command::new(perry_bin());
    command
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output);
    if let Some(platform) = platform {
        command.arg("--platform").arg(platform);
    }
    let compile = command.output().expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    output
}

fn run(output: &Path, dir: &Path) -> String {
    let run = Command::new(output)
        .current_dir(dir)
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
fn node_platform_keeps_bun_global_absent() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("main.ts"),
        r#"
console.log(typeof Bun);
console.log(typeof globalThis.Bun);
console.log(typeof globalThis["Bun"]);
"#,
    )
    .expect("write entry");

    let output = compile(dir.path(), None);
    assert_eq!(
        run(&output, dir.path()),
        "undefined\nundefined\nundefined\n"
    );
}

#[test]
fn bun_platform_installs_one_shared_namespace_for_every_access_form() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("dependency.ts"),
        r#"
export const dependencySawBun = globalThis.Bun === Bun;
console.log("dependency", typeof Bun, dependencySawBun);
"#,
    )
    .expect("write dependency");
    std::fs::write(
        dir.path().join("main.ts"),
        r#"
import { dependencySawBun } from "./dependency.ts";

console.log(typeof Bun);
console.log(Bun === globalThis.Bun);
console.log(Bun === globalThis["Bun"]);
console.log(dependencySawBun);

console.log(typeof Bun.hash);
const extracted = Bun.stringWidth;
console.log(extracted("abc"));
const { stringWidth } = Bun;
console.log(stringWidth("abcd"));
const key = "file";
console.log(typeof Bun[key]);
console.log(Bun?.stringWidth?.("abcde"));

console.log(typeof Bun.version, Bun.version.length > 0);
console.log(Bun.isStandaloneExecutable);
console.log(typeof Bun.notImplemented);
const keys = Object.keys(Bun);
console.log(keys.includes("stringWidth"), keys.includes("version"));

try {
  Bun.serve();
} catch (error: any) {
  console.log(error.message === "Bun.serve is not supported by Perry");
}

function scoped() {
  const Bun = { stringWidth: (_value: string) => 99 };
  return Bun.stringWidth("shadowed");
}
console.log(scoped());
"#,
    )
    .expect("write entry");

    let output = compile(dir.path(), Some("bun"));
    let expected = "\
dependency object true
object
true
true
true
function
3
4
function
5
string true
true
undefined
true true
true
99
";
    assert_eq!(run(&output, dir.path()), expected);
}
