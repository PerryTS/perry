//! #10925 smoke test (NOT a proof): the SAB's `GcHeader` must survive heavy
//! multi-thread collection.
//!
//! The source audit (PR body / plan L15.7) shows no collector path WRITES a
//! SAB header — every mark/move/sweep gates on this-thread arena or
//! malloc-tracked membership, which a process-global SAB is in on no thread.
//! This backs that empirically: the main thread and two workers each allocate
//! enough to force several minor and major collections while all three hold
//! the same SAB and run `Atomics` traffic on it. If any collector wrote the
//! header (a mark bit, a stale forward), the bytes would move and a later
//! typed-array read over the SAB would see corruption or the program would
//! crash. A clean, node-matching run across many collections is the signal.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn a_sab_header_survives_heavy_multithread_collection() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
import { spawn } from "perry/thread";
const sab = new SharedArrayBuffer(64);
const cell = new Int32Array(sab);
cell[0] = 0;
function churn(rounds: number): void {
  for (let r = 0; r < rounds; r++) {
    let junk: any[] = [];
    for (let i = 0; i < 20000; i++) junk.push({ a: i, b: [i, i + 1], c: "s" + i });
    junk = [];
    Atomics.add(cell, 0, 1);
  }
}
async function main() {
  const w1 = spawn(() => {
    const v = new Int32Array(sab);
    for (let r = 0; r < 40; r++) {
      let j: any[] = [];
      for (let i = 0; i < 20000; i++) j.push({ x: i, y: "" + i });
      j = [];
      Atomics.add(v, 1, 1);
    }
    return Atomics.load(v, 1);
  });
  const w2 = spawn(() => {
    const v = new Int32Array(sab);
    for (let r = 0; r < 40; r++) {
      let j: any[] = [];
      for (let i = 0; i < 20000; i++) j.push([i, i, i]);
      j = [];
      Atomics.add(v, 2, 1);
    }
    return Atomics.load(v, 2);
  });
  churn(40);
  const a = await w1;
  const b = await w2;
  // Every worker's and the main thread's Atomics counters landed in the one
  // shared buffer, and the buffer is still a SharedArrayBuffer afterwards.
  console.log("main", Atomics.load(cell, 0));
  console.log("w1", a, "w2", b);
  console.log("shared-w1", Atomics.load(cell, 1), "shared-w2", Atomics.load(cell, 2));
  console.log("brand", Object.prototype.toString.call(sab));
  console.log("isArray", Array.isArray(sab));
  console.log("len", sab.byteLength);
}
main();
"#,
    )
    .unwrap();
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("compile");
    assert!(
        compile.status.success(),
        "compile failed\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&output)
        .current_dir(dir.path())
        .output()
        .expect("run");
    assert!(
        run.status.success(),
        "a signal here would be a collector writing the shared SAB header\nstatus {:?}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "main 40\n\
         w1 40 w2 40\n\
         shared-w1 40 shared-w2 40\n\
         brand [object SharedArrayBuffer]\n\
         isArray false\n\
         len 64\n"
    );
}
