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

/// Closure created INSIDE the labeled block, reassignment outside. Root
/// cause of the former residual (un-ignored by the fix): a BOXED local's
/// declared type stayed in `module_local_types`, so the typed-ABI closure
/// specialization (`__typed_f64`) read the capture RAW while the generic
/// variant went through `js_box_get` — and the dispatcher picked the typed
/// body, returning box-pointer bits as a denormal. Boxed ids are now
/// filtered out of `module_local_types`, so boxed captures always take the
/// generic (box-aware) paths.
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
