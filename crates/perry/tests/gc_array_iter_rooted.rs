//! Regression test for the rooted array-iteration rework (2026-07-02 audit,
//! GC deep set): the higher-order array methods hoisted `elements_ptr` (and
//! map/filter/flatMap their result pointers, reduce its accumulator) across
//! user callbacks. A callback-triggered MOVING collection relocates the
//! array — elements live inline after the header — so the hoisted pointer
//! read from-space garbage. Every method now re-derives through a
//! `RuntimeHandleScope` root per iteration.
//!
//! The program below calls `gc()` (a manual, full, potentially-moving
//! collection) inside EVERY callback, plus allocation pressure. Expected
//! output is byte-for-byte what `node --experimental-strip-types
//! --expose-gc` prints for the same program.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn array_iteration_survives_gc_in_every_callback() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
const a = [1, 2, 3, 4, 5];
const big = () => { let x: any[] = []; for (let i = 0; i < 2000; i++) x.push({ i }); return x.length; };
const g = (globalThis as any).gc;
const mapped = a.map((v, i) => { big(); g(); return v * 10 + i; });
console.log("map:", mapped.join(","));
const filtered = a.filter((v) => { g(); return v % 2 === 1; });
console.log("filter:", filtered.join(","));
let sum = 0;
a.forEach((v) => { g(); sum += v; });
console.log("forEach:", sum);
console.log("find:", a.find((v) => { g(); return v === 4; }));
console.log("findIndex:", a.findIndex((v) => { g(); return v === 4; }));
console.log("findLast:", (a as any).findLast((v: number) => { g(); return v < 4; }));
console.log("some:", a.some((v) => { g(); return v > 4; }));
console.log("every:", a.every((v) => { g(); return v > 0; }));
console.log("flatMap:", a.flatMap((v) => { g(); return [v, v * 2]; }).join(","));
console.log("reduce:", a.reduce((acc, v) => { g(); return acc + "|" + v; }, "R"));
console.log("reduceRight:", a.reduceRight((acc, v) => { g(); return acc + "|" + v; }, "L"));
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
        "map: 10,21,32,43,54\n\
         filter: 1,3,5\n\
         forEach: 15\n\
         find: 4\n\
         findIndex: 3\n\
         findLast: 3\n\
         some: true\n\
         every: true\n\
         flatMap: 1,2,2,4,3,6,4,8,5,10\n\
         reduce: R|1|2|3|4|5\n\
         reduceRight: L|5|4|3|2|1\n"
    );
}
