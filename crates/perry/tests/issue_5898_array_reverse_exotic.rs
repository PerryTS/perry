//! Regression coverage for the Array.prototype.reverse exotic-index cluster
//! in #5898. Reverse must observe inherited indices and getter side effects in
//! specification order instead of swapping raw dense slots.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn reverse_observes_inherited_indices_and_live_presence() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
declare function gc(): void;

(Array.prototype as any)[1] = 1;
const inherited = [0];
inherited.length = 2;
inherited.reverse();
console.log("inherited", inherited[0], inherited[1]);
delete (Array.prototype as any)[1];

const live = ["first", "second"];
Object.defineProperty(live, 0, {
  configurable: true,
  get() {
    live.length = 0;
    return "first";
  }
});
live.reverse();
console.log("live", 0 in live, 1 in live, live[1]);

const sparse: any[] = [];
const sparseIndex = 1_001_025;
sparse[sparseIndex] = "far";
sparse.length = 0;
console.log("sparse", sparse.length, sparseIndex in sparse);

// A sparse named-property entry can become covered by dense capacity after a
// push grows the backing store. Force a collection so the side-table owner is
// rekeyed from the growth forwarding stub, then make sure deletion clears both
// storage representations.
const regrown: any[] = [];
const farIndex = sparseIndex;
regrown[farIndex] = "far";
regrown.push("tail");
gc();
delete regrown[farIndex];
console.log(
  "regrown",
  Object.prototype.hasOwnProperty.call(regrown, farIndex),
  farIndex in regrown
);
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
        "inherited 1 0\nlive false true first\nsparse 0 false\nregrown false false\n"
    );
}
