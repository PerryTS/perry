//! Regression test: a `for await` over an *untyped* async iterable leaked the
//! lowering context's `inside_block_scope` counter.
//!
//! `lower_stmt_for_of` / `lower_body_stmt` open a block scope
//! (`push_block_scope`) for the loop, but for the async-iterator runtime path
//! they `return lower_runtime_for_await_iterator(...)` early — and that callee
//! manages its own block scope, so the scope opened for the loop was never
//! popped. The +1 leak escaped the enclosing function boundary because
//! `enter_scope`/`exit_scope` do not save/restore `inside_block_scope`.
//!
//! A later module-level `var X = <expr>` then saw `inside_block_scope != 0`, so
//! the #1758 pre-registration-reuse gate (`destructuring/var_decl.rs`) skipped
//! reuse and allocated a *fresh* LocalId. The pre-scanned id that a
//! forward-referencing sibling closure had already bound was orphaned and never
//! written, so calling that closure's reference threw
//! `TypeError: value is not a function`.
//!
//! This is the wall that blocked the minified `@anthropic-ai/claude-code`
//! bundle (12 leaked `for await` loops → `inside_block_scope == 12` at the
//! `e8`/`K8` module-var declarations). Fix: pop the loop's block scope before
//! the async-iterator early returns.

use std::path::PathBuf;
use std::process::Command;

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
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
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
fn for_await_does_not_orphan_forward_referenced_module_var() {
    let dir = tempfile::tempdir().expect("tempdir");
    // `e8` forward-references `K8` (declared later); a `for await` over an
    // untyped async iterable sits between, leaking the block-scope counter.
    // Pre-fix, `e8()` read an orphaned (undefined) `K8` slot → "K8 is not a
    // function"; post-fix it resolves the real wrapper.
    let stdout = compile_and_run(
        dir.path(),
        r#"
var L = (q: any, K?: any) => () => (q && (K = q((q = 0) as any)), K);
async function leak() {
  for await (const x of (async function* () {})()) { void x; }
}
var e8 = L(() => "K8type:" + typeof K8);
var K8 = L(() => "ok");
console.log(e8());
"#,
    );
    assert_eq!(stdout, "K8type:function\n");
}

#[test]
fn for_await_over_untyped_async_iterable_still_iterates() {
    // The fix must not break actual async iteration.
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
async function main() {
  async function* gen() { yield 1; yield 2; yield 3; }
  let sum = 0;
  for await (const x of (gen() as any)) { sum += x; }
  console.log("sum", sum);
}
main();
"#,
    );
    assert_eq!(stdout, "sum 6\n");
}
