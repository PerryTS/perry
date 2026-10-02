//! Every class declaration is its own class, whatever its name. Two blocks
//! that each declare `class B {}` / `class C extends B {}` declare four
//! classes; a write to the second block's `B.prototype` must be seen by that
//! block's `C` instances and by nothing in the first block. The program and
//! node's output are the `block_class_identity` fixture; it also covers
//! methods, fields and statics on same-named classes in sibling blocks, a
//! function whose body declares same-named classes in two blocks, a loop body
//! and nested blocks shadowing an outer class, `instanceof` across the
//! same-named pairs, class expressions sharing a name, and same-named classes
//! exported by two modules.
//!
//! A shadowing `class X` registers under a scope-local key (`class_renames`,
//! #9466). The `X.prototype.m = v` recognizer looked the class up by its raw
//! source name, so it registered the write on the OTHER `X`: the second
//! block's `B.prototype.j = 6` patched the first block's `B`, and the reported
//! program printed `undefined` where node prints `6`.

use std::path::PathBuf;
use std::process::Command;

const DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/block_class_identity"
);
const EXPECTED: &str = include_str!("../../../tests/fixtures/block_class_identity/expected.txt");

#[test]
fn same_named_classes_keep_their_own_identity() {
    let dir = tempfile::tempdir().expect("tempdir");
    for file in ["main.ts", "mod_a.ts", "mod_b.ts"] {
        std::fs::copy(PathBuf::from(DIR).join(file), dir.path().join(file))
            .unwrap_or_else(|e| panic!("copy {file}: {e}"));
    }
    let output = dir.path().join("main_bin");
    let compile = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_perry")))
        .current_dir(dir.path())
        .arg("compile")
        .arg(dir.path().join("main.ts"))
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_CACHE", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&output)
        .current_dir(dir.path())
        .env("PERRY_GC_FORCE_EVACUATE", "1")
        .env("PERRY_GC_POISON_FROMSPACE", "1")
        .output()
        .expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    // Exit status and stderr first: a throw part-way through leaves a prefix
    // of correct lines that a line-by-line compare alone would not flag.
    assert!(
        run.status.success() && stderr.trim().is_empty(),
        "binary failed ({:?})\nstdout:\n{stdout}\nstderr:\n{stderr}",
        run.status
    );
    let wrong: Vec<String> = EXPECTED
        .lines()
        .zip(stdout.lines())
        .filter(|(want, got)| want != got)
        .map(|(want, got)| format!("got `{got}`, node `{want}`"))
        .collect();
    assert!(
        wrong.is_empty() && EXPECTED.lines().count() == stdout.lines().count(),
        "same-named classes diverge from node: {wrong:?}\n{stdout}"
    );
}
