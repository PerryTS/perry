//! Regression test for #4926 (source bug behind the #4898 SIGBUS): a boxed
//! (closure-captured + mutated) variable read or written on a control-flow
//! path where its `let` initializer never executed must be DEFINED behavior.
//!
//! Before the fix, the boxed slot's alloca lived in the entry block but the
//! box-pointer store only ran when the `Stmt::Let` executed — a skipped-init
//! path loaded an uninitialized slot, LLVM folded it to `undef`, and regalloc
//! substituted whatever register was live. Under typed-feedback density
//! (react-reconciler) that was a read-only guard-string constant, and
//! `js_box_set` wrote into `__TEXT.__cstring` → SIGBUS. The fix entry-
//! initializes every boxed slot with TAG_UNDEFINED, so skipped-init reads see
//! `undefined` (Perry has no TDZ) and skipped-init writes are deterministic
//! no-ops.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn boxed_var_skipped_init_is_defined_behavior() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");

    // A switch-case `let` is in scope for the whole switch block, but its
    // initializer only runs when its case is taken — entering at case 2
    // reaches boxed reads/writes with no dominating box allocation.
    std::fs::write(
        &entry,
        r#"
function readSkipped(c: number): any {
  switch (c) {
    case 1:
      let x: any = 5;
      const inc = () => {
        x = x + 1;
      };
      inc();
      return x;
    case 2:
      return typeof x;
  }
  return "none";
}
console.log(readSkipped(2));
console.log(readSkipped(1));
console.log(readSkipped(3));

function writeSkipped(c: number): any {
  switch (c) {
    case 1:
      let y: any = 5;
      const incY = () => {
        y = y + 1;
      };
      incY();
      return y;
    case 2:
      y = 99;
      return typeof y;
  }
  return "none";
}
console.log(writeSkipped(2));
console.log(writeSkipped(1));
"#,
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
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

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed (signal = #4926/#4898 regression)\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    // Skipped-init read = "undefined" (pre-fix: undef garbage, typically
    // "number" via a rejected-pointer NaN). Taken paths unchanged.
    // Skipped-init write is a deterministic no-op, so the follow-up read is
    // still "undefined".
    assert_eq!(
        stdout, "undefined\n6\nnone\nundefined\n6\n",
        "boxed skipped-init paths must be deterministic undefined"
    );
}
