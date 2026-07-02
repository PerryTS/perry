//! Regression test for #5484 — a TypedArray's iterating methods must work even
//! when the array's backing landed below `clean_arr_ptr`'s (macOS) 2 TB heap
//! window.
//!
//! Typed arrays are old-arena allocations (`arena_alloc_gc_old`). On macOS they
//! can land below `clean_arr_ptr`'s `HEAP_MIN` floor, where it would null the
//! receiver — while `clean_ta_ptr` (4 KB floor) accepts the same address. So
//! `.length`/index access worked but every `Array.prototype` iterating method
//! (`reduce`/`forEach`/`map`/`join`/`indexOf`, which funnel through
//! `normalize_array_receiver`) silently saw an empty array. A preceding
//! `structuredClone(<non-empty object>)` reliably shifts allocation so the
//! typed array lands in that range.
//!
//! Fix: `normalize_array_receiver` returns registered typed arrays / buffers
//! un-nulled (registry membership is the real liveness check), so the caller's
//! typed-array dispatch fires.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn run_ts(src: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(&entry, src).expect("write");
    let out = dir.path().join("bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&out)
        .output()
        .expect("compile");
    assert!(
        compile.status.success(),
        "compile failed:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&out).output().expect("run");
    assert!(
        run.status.success(),
        "binary exited with {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).to_string()
}

#[test]
fn typed_array_iter_methods_work_after_structured_clone() {
    let stdout = run_ts(
        r#"
// structuredClone of a non-empty object shifts allocation so the following
// typed array lands below clean_arr_ptr's macOS heap floor (the #5484 trigger).
const sc = structuredClone({ a: 1, b: [2, 3] })

const f = new Float64Array([1.5, 2.5, 3.5])
console.log("length:", f.length)              // unaffected before the fix
console.log("reduce:", f.reduce((a, b) => a + b, 0))
console.log("join:", f.join(","))
console.log("indexOf:", f.indexOf(2.5))
let s = 0
f.forEach((x) => { s += x })
console.log("forEach:", s)
console.log("map:", Array.from(f.map((x) => x * 2)).join(","))

const i = new Int32Array([10, 20, 30])
console.log("i32reduce:", i.reduce((a, b) => a + b, 0))
"#,
    );

    for needle in [
        "length: 3",
        "reduce: 7.5",
        "join: 1.5,2.5,3.5",
        "indexOf: 1",
        "forEach: 7.5",
        "map: 3,5,7",
        "i32reduce: 60",
    ] {
        assert!(stdout.contains(needle), "missing `{needle}` in:\n{stdout}");
    }
}
