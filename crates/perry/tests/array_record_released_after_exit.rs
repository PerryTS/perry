//! Perry-only retention check for the for-of exit release (PR #12281).
//!
//! A for-of over an array must stop keeping its source reachable once the
//! loop ends, by exhaustion, break, throw or labeled break, while the frame
//! that ran it stays live. The compiled program asks each source's WeakRef
//! after a forced Perry collection. This is not compared with Node: V8 may
//! keep the source alive in its frame, so the observation is not a parity
//! property. The codegen test `array_stack_record_exit_releases_its_payload_home`
//! covers the store itself; this runs the program.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn array_source_is_released_after_every_loop_exit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
declare function gc(): void;

const box: { [k: string]: any[] | undefined } = {};
const probes: { [k: string]: WeakRef<any[]> } = {};
for (const kind of ["normal", "break", "throw", "labeled"]) {
  const source: any[] = [];
  for (let i = 0; i < 2000; i++) source.push({ i, kind });
  box[kind] = source;
  probes[kind] = new WeakRef(source);
}

function take(kind: string): any[] {
  const source = box[kind]!;
  box[kind] = undefined;
  return source;
}

function churn() {
  let junk: any[] = [];
  for (let i = 0; i < 20000; i++) {
    junk.push({ i });
    if (junk.length > 100) junk = [];
  }
}

function consume(kind: string): string {
  let sum = 0;
  if (kind === "normal") {
    for (const v of take(kind)) sum += v.i;
  } else if (kind === "break") {
    for (const v of take(kind)) {
      if (v.i === 7) break;
      sum += v.i;
    }
  } else if (kind === "throw") {
    try {
      for (const v of take(kind)) {
        if (v.i === 11) throw new Error("stop");
        sum += v.i;
      }
    } catch (e) {
      sum += 1000;
    }
  } else {
    outer: for (let round = 0; round < 3; round++) {
      for (const v of take(kind)) {
        if (v.i === 3) break outer;
        sum += v.i;
      }
    }
  }
  // This frame is still live and the loop is done.
  churn();
  gc();
  return kind + " " + sum + " released: " + (probes[kind].deref() === undefined);
}

async function main() {
  await new Promise((r) => setTimeout(r, 0));
  console.log(consume("normal"));
  console.log(consume("break"));
  console.log(consume("throw"));
  console.log(consume("labeled"));
}
main();
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
        "perry compile failed\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed (exit {:?})\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "normal 1999000 released: true\n\
         break 21 released: true\n\
         throw 1055 released: true\n\
         labeled 3 released: true\n"
    );
}
