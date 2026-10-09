//! #10812's prologue stack check, as emitted and as it behaves.
//!
//! Every compiled JS body starts with a compare of the stack pointer against
//! the agent's stack limit (`perry-codegen/src/expr/stack_guard.rs`). On an
//! ELF x86-64 executable that is two instructions,
//! `cmp %fs:PERRY_AGENT_PTRS@TPOFF+16, %rsp; jb`, and the overflow arm is a
//! call to the `cold` `js_stack_overflow` that ends the block: nothing is
//! live across it, so no spill before it and no reload or jump back after.
//!
//! The behavioural half runs a deep recursion in a plain function, a method,
//! an async function, a generator and a Worker agent, and requires each to
//! throw a catchable `RangeError: Maximum call stack size exceeded` and the
//! program to keep running. A guard that compares the wrong limit lets the
//! recursion run into the guard page instead, and this test fails.

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const MAIN: &str = r#"
import { Worker } from 'node:worker_threads';

function down(n: number): number {
  return down(n + 1) + 1;
}
try {
  down(0);
  console.log("function: no throw");
} catch (e: any) {
  console.log("function:", e instanceof RangeError, e.message);
}

class Walker {
  walk(n: number): number {
    return this.walk(n + 1) + 1;
  }
}
try {
  new Walker().walk(0);
} catch (e: any) {
  console.log("method:", e instanceof RangeError, e.message);
}

// The overflow is raised under a generator body's frame and leaves it
// through `next()`. (Not a recursion through nested generator resumes or
// async bodies: each holds a `try` handler, and the runtime's handler stack,
// 1024 deep, runs out before the native stack does.)
function* nest(n: number): Generator<number> {
  yield down(n);
}
try {
  nest(0).next();
  console.log("generator: no throw");
} catch (e: any) {
  console.log("generator:", e instanceof RangeError, e.message);
}
const again = nest(0);
try {
  again.next();
} catch (e: any) {
  console.log("generator done:", again.next().done);
}

// The overflow is raised under an async body's frame and rejects its
// promise.
async function downAsync(n: number): Promise<number> {
  return down(n) + (await Promise.resolve(1));
}

downAsync(0)
  .then(() => console.log("async: no throw"))
  .catch((e: any) => console.log("async:", e instanceof RangeError, e.message))
  .then(() => {
    const worker = new Worker(new URL("./sg_worker.ts", import.meta.url));
    worker.on("message", (m: string) => {
      console.log("worker:", m);
      worker.terminate().then(() => console.log("after:", down.length));
    });
  });
"#;

const WORKER: &str = r#"
import { parentPort } from 'node:worker_threads';
function deeper(n: number): number {
  return deeper(n + 1) + 1;
}
let report = "no throw";
try {
  deeper(0);
} catch (e: any) {
  report = `${e instanceof RangeError} ${e.message}`;
}
parentPort!.postMessage(report);
"#;

const EXPECTED: &str = "\
function: true Maximum call stack size exceeded
method: true Maximum call stack size exceeded
generator: true Maximum call stack size exceeded
generator done: true
async: true Maximum call stack size exceeded
worker: true Maximum call stack size exceeded
after: 1
";

fn compile(dir: &Path, extra_env: &[(&str, &Path)]) -> PathBuf {
    std::fs::write(dir.join("sg_main.ts"), MAIN).expect("write main");
    std::fs::write(dir.join("sg_worker.ts"), WORKER).expect("write worker");
    let exe = dir.join("sg_main");
    let mut cmd = Command::new(perry_bin());
    cmd.current_dir(dir)
        .arg("compile")
        .arg(dir.join("sg_main.ts"))
        .arg("-o")
        .arg(&exe)
        .arg("--no-cache")
        .env("PERRY_KEEP_SYMBOLS", "1");
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("run perry compile");
    assert!(
        out.status.success(),
        "compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    exe
}

#[test]
fn deep_recursion_throws_a_catchable_range_error_in_every_frame_kind() {
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = compile(dir.path(), &[]);
    let out = Command::new(&exe)
        .current_dir(dir.path())
        .output()
        .expect("run the program");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout == EXPECTED,
        "status {:?}\nstdout:\n{stdout}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );
}

/// The body of the `define` whose line contains `needle`.
fn function_body<'a>(ir: &'a str, needle: &str) -> Option<&'a str> {
    let start = ir
        .match_indices("\ndefine ")
        .map(|(i, _)| i + 1)
        .find(|&i| ir[i..].lines().next().is_some_and(|l| l.contains(needle)))?;
    let rest = &ir[start..];
    let end = rest.find("\n}\n").map(|i| i + 3).unwrap_or(rest.len());
    Some(&rest[..end])
}

/// The emitted IR: the stack pointer through `llvm.read_register`, the
/// limit through the local-exec agent block, and an overflow arm that ends
/// in `unreachable` after its `cold` call.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn the_prologue_check_is_one_compare_and_an_arm_that_ends_in_the_ir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ll = dir.path().join("ll");
    std::fs::create_dir_all(&ll).expect("ll dir");
    compile(dir.path(), &[("PERRY_SAVE_LL", &ll)]);
    let ir: String = std::fs::read_dir(&ll)
        .expect("read ll dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "ll"))
        .map(|e| std::fs::read_to_string(e.path()).expect("read .ll"))
        .collect();
    assert!(
        ir.contains("@PERRY_AGENT_PTRS = external thread_local(localexec) global"),
        "the agent block must be local-exec in an executable:\n{}",
        ir.lines()
            .filter(|l| l.contains("PERRY_AGENT_PTRS"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(
        ir.contains("declare void @js_stack_overflow() cold"),
        "js_stack_overflow must be declared cold"
    );
    // The public body (its `$spec` recursion clone checks before its calls).
    let down = function_body(&ir, "@perry_fn_sg_main_ts__down(")
        .unwrap_or_else(|| panic!("no down():\n{ir}"));
    assert!(
        down.contains("call i64 @llvm.read_register.i64(metadata !{!\"rsp\"})"),
        "the stack pointer must be read with llvm.read_register:\n{down}"
    );
    let arm = down
        .split_once("\nstack_guard.overflow")
        .map(|(_, arm)| arm)
        .unwrap_or_else(|| panic!("no overflow arm:\n{down}"));
    let mut lines = arm.lines().skip(1).map(str::trim).filter(|l| !l.is_empty());
    assert_eq!(
        lines.next(),
        Some("call void @js_stack_overflow()"),
        "{down}"
    );
    assert_eq!(lines.next(), Some("unreachable"), "{down}");
}

/// The machine code: `cmp %fs:..., %rsp` then `jb`, with no stack-pointer
/// copy before it, and nothing executed after the overflow call.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn the_prologue_check_is_two_instructions_in_the_binary() {
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = compile(dir.path(), &[]);
    let nm = Command::new("nm").arg(&exe).output().expect("run nm");
    let symbols = String::from_utf8_lossy(&nm.stdout);
    let down = symbols
        .lines()
        .filter_map(|l| l.split_whitespace().nth(2))
        .find(|s| s.starts_with("perry_fn_") && s.ends_with("sg_main_ts__down"))
        .unwrap_or_else(|| panic!("no down() symbol in:\n{symbols}"))
        .to_string();
    let dis = Command::new("objdump")
        .args(["-d", "--no-show-raw-insn", &format!("--disassemble={down}")])
        .arg(&exe)
        .output()
        .expect("run objdump");
    let text = String::from_utf8_lossy(&dis.stdout);
    let insns: Vec<&str> = text
        .lines()
        .filter_map(|l| l.split_once(":\t").map(|(_, i)| i.trim()))
        .collect();
    let at = insns
        .iter()
        .position(|i| i.starts_with("cmp") && i.contains("%fs:") && i.ends_with(",%rsp"))
        .unwrap_or_else(|| panic!("no `cmp %fs:..,%rsp` in down():\n{text}"));
    assert!(
        insns.get(at + 1).is_some_and(|i| i.starts_with("jb")),
        "the compare must branch with jb:\n{text}"
    );
    assert!(
        !insns[..at]
            .iter()
            .any(|i| i.starts_with("mov") && i.contains("%rsp,") && !i.ends_with(",%rbp")),
        "no stack-pointer copy before the check:\n{text}"
    );
    let call = insns
        .iter()
        .position(|i| i.starts_with("call") && i.contains("<js_stack_overflow"))
        .unwrap_or_else(|| panic!("no overflow call in down():\n{text}"));
    let is_move = |i: &&&str| i.starts_with("mov") || i.starts_with("vmov");
    assert!(
        !insns[..call]
            .last()
            .is_some_and(|i| is_move(&i) && i.ends_with("(%rbp)")),
        "no spill right before the overflow call:\n{text}"
    );
    assert!(
        insns
            .get(call + 1)
            .is_none_or(|i| !(i.starts_with("jmp") || (is_move(&i) && i.contains("(%rbp),")))),
        "nothing resumes after the overflow call:\n{text}"
    );
}
