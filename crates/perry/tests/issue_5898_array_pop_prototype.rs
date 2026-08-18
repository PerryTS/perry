//! Regression coverage for the Array.prototype.pop subcluster in #5898.
//! Pop must use ordinary property access for the last index, then re-check
//! receiver state after an inherited getter has run.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn pop_reads_inherited_last_index_and_observes_getter_side_effects() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
(Array.prototype as any)[1] = 1;
const inherited = [0];
inherited.length = 2;
console.log("inherited", inherited.pop(), inherited.length, (Array.prototype as any)[1]);
delete (Array.prototype as any)[1];

const frozen: any[] = [];
frozen.length = 1;
Object.defineProperty(Array.prototype, "0", {
  configurable: true,
  get() { Object.freeze(frozen); return 7; }
});
try {
  frozen.pop();
  console.log("frozen no throw");
} catch (error) {
  console.log("frozen", error instanceof TypeError, frozen.length);
}
delete (Array.prototype as any)[0];

const readonlyLength: any[] = [];
readonlyLength.length = 1;
Object.defineProperty(Array.prototype, "0", {
  configurable: true,
  get() {
    Object.defineProperty(readonlyLength, "length", { writable: false });
    return 8;
  }
});
try {
  readonlyLength.pop();
  console.log("readonly no throw");
} catch (error) {
  console.log("readonly", error instanceof TypeError, readonlyLength.length);
}
delete (Array.prototype as any)[0];
"#,
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
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
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "inherited 1 1 1\nfrozen true 1\nreadonly true 1\n"
    );
}
