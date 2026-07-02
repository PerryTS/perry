//! Regression tests for #5869: a captured variable reassigned (or captured)
//! inside a LABELED block was not boxed, because five walkers in
//! perry-codegen's boxing analysis (`boxed_vars.rs`) had no `Stmt::Labeled`
//! arm — a reassignment inside a labeled block was invisible to
//! `outer_writes`, and a closure created inside one contributed no
//! `closure_refs`. The closure then captured a creation-time snapshot and
//! never observed later writes: reads through the closure saw a stale value
//! while direct reads in the same scope saw the live one.
//!
//! Minified bundles emit labeled early-exit blocks (`e: { … break e; }`)
//! pervasively, which is why this class only reproduced in large bundles.
//!
//! Expected outputs are byte-for-byte what `node --experimental-strip-types`
//! prints.

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
        .arg("--no-cache")
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
        "compiled binary failed (exit {:?})\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// Reassignment INSIDE the labeled block, closure created outside. Pre-fix
/// the write was invisible to `collect_outer_writes` → `arr` unboxed → the
/// closure read the stale creation-time empty array (`len: 0`) while
/// JSON.stringify in the same scope printed all four elements — the exact
/// signature previously seen only in large webpack bundles.
#[test]
fn write_inside_labeled_block_visible_to_capture() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function factory() {
  let arr: any = [];
  const read = () => arr.length;
  outer: {
    arr = [1, 2, 3, 4];
    break outer;
  }
  console.log("len:", read());
  console.log("json:", JSON.stringify(arr));
}
factory();
"#,
    );
    assert_eq!(
        stdout, "len: 4\njson: [1,2,3,4]\n",
        "a reassignment inside a labeled block must box the captured var"
    );
}

/// Closure created INSIDE the labeled block, reassignment outside. The
/// codegen-side walkers now see it, but perry-hir's lowering still
/// classifies the capture as non-mutable (`--trace hir` shows the closure
/// under `Labeled { body: DoWhile }` with `mutable_captures: []`), so the
/// creation site stores the box pointer while the body reads it raw — the
/// call returns a denormal number (box-pointer bits). Tracked as the
/// residual half of #5869; un-ignore when the HIR mutable-capture
/// classifier learns the Labeled wrapper.
#[ignore = "residual #5869: HIR mutable_captures misses closures under Labeled wrappers"]
#[test]
fn closure_inside_labeled_block_counts_as_capture() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function factory2() {
  let n = 0;
  let get: any = null;
  tag: {
    get = () => n;
    break tag;
  }
  n = 42;
  console.log("n:", get());
}
factory2();
"#,
    );
    assert_eq!(
        stdout, "n: 42\n",
        "a closure created inside a labeled block must count toward boxing"
    );
}
