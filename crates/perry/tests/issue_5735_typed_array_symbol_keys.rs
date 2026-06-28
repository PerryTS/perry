//! Regression test for #5735 (cluster 1): Symbol-keyed property writes on a
//! *statically-typed* typed array (`const t = new Float64Array(2); t[sym] = v`).
//!
//! A statically-typed multi-byte typed-array receiver lowers through the
//! width-tracked native store path (`lower_typed_array_store`), which coerced
//! the index with `fptosi`. A NaN-boxed Symbol truncates to index 0, so
//! `t[sym] = v` silently *clobbered element 0* instead of storing a symbol
//! property — corrupting the array's data and dropping the symbol. The fix
//! routes non-numeric keys on the width-tracked store path to the symbol-aware
//! runtime dispatcher (mirroring the symmetric IndexGet guard), and makes the
//! `js_typed_array_index_{get,set}_dynamic` helpers triage Symbol keys into the
//! symbol side table. This test pins that a Symbol write lands in the symbol
//! table, leaves the element data untouched, and survives `reverse()` /
//! `Reflect.ownKeys` ordering — no test262 harness needed, so it runs in CI.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn typed_array_symbol_keys_are_stored_not_coerced_to_element_zero() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");

    std::fs::write(
        &entry,
        r#"
const s1 = Symbol("1");
const s2 = Symbol("2");

// (A) A symbol write on a statically-typed multi-byte typed array must NOT
// clobber element 0; it lands in the symbol side table and reads back.
const f = new Float64Array(3);
f[0] = 11; f[1] = 22; f[2] = 33;
f[s1] = 99;
console.log("A=" + f[s1] + "," + f[0] + "," + f[1] + "," + f[2]); // 99,11,22,33

// (B) Multiple symbols + string expandos: all retained, and Reflect.ownKeys
// orders integer indexes, then string keys (insertion order), then symbols.
const g = new Int16Array(2);
(g as any).foo = 42;
g[s1] = 1;
g[s2] = 2;
console.log("B=" + Object.getOwnPropertySymbols(g).length);                 // 2
const keys = Reflect.ownKeys(g).map(String).join(",");
console.log("Bkeys=" + keys);                                               // 0,1,foo,Symbol(1),Symbol(2)

// (C) reverse() preserves symbol-keyed (and string-keyed) properties while
// reversing the elements.
const r = new Int8Array(2);
r[0] = 5; r[1] = 6;
(r as any).bar = "bar";
r[s1] = 7;
const rr = r.reverse();
console.log("C=" + rr[s1] + "," + (rr as any).bar + "," + rr[0] + "," + rr[1]); // 7,bar,6,5

// (D) Symbol read on a fresh typed array (no prior write) is undefined, and a
// numeric index still reads the element (the guard only diverts non-numerics).
const h = new Uint32Array(1);
h[0] = 1234;
console.log("D=" + (h[s2] === undefined) + "," + h[0]);                      // true,1234

console.log("DONE");
"#,
    )
    .expect("write entry");

    let compiled = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compiled.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compiled.stdout),
        String::from_utf8_lossy(&compiled.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    let out = String::from_utf8_lossy(&run.stdout).to_string();
    assert!(run.status.success(), "binary did not exit cleanly\n{out}");

    assert!(
        out.contains("A=99,11,22,33"),
        "symbol write clobbered element data\n{out}"
    );
    assert!(out.contains("B=2"), "both symbols must be retained\n{out}");
    assert!(
        out.contains("Bkeys=0,1,foo,Symbol(1),Symbol(2)"),
        "ownKeys ordering (indexes, strings, symbols)\n{out}"
    );
    assert!(
        out.contains("C=7,bar,6,5"),
        "reverse must preserve symbol + reverse elements\n{out}"
    );
    assert!(
        out.contains("D=true,1234"),
        "symbol read undefined; numeric read intact\n{out}"
    );
    assert!(out.contains("DONE"), "program did not finish\n{out}");
}
