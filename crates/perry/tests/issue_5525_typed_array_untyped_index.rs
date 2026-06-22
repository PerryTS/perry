//! Regression test for #5525: typed-array element access through an *untyped*
//! receiver (the shape bcryptjs's Blowfish core uses — its `Int32Array` P/S
//! boxes reach `_encipher`/`_key` as plain `Array.<number>` parameters).
//!
//! Such accesses lower to the runtime `js_dyn_index_get` / `js_dyn_index_set`
//! dispatchers, which previously resolved "is this a typed array, and of what
//! kind" through the thread-local `TYPED_ARRAY_REGISTRY` on *every* element —
//! ~5 thread-local lookups per read down a deep dispatch chain. At ~600M reads
//! for one cost-12 `bcrypt.compareSync` that turned a ~50ms operation into ~28s
//! (mis-reported as an infinite-loop hang in #5525). The fix adds a process-
//! global kind cache plus an inline owning-typed-array fast path. This test
//! pins the *behavioural* correctness of that fast path across element kinds,
//! bounds, exotic keys, BigInt kinds, and ArrayBuffer-view aliasing — no
//! bcryptjs dependency, so it runs in CI without `node_modules`.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn typed_array_untyped_index_access_is_correct() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");

    std::fs::write(
        &entry,
        r#"
// Read/write a typed array reached only through untyped params — the access
// site cannot know the static type, so it goes through the dynamic dispatcher.
function get(a: any, i: number): any { return a[i]; }
function set(a: any, i: number, v: any): void { a[i] = v; }

// (A) Int32Array with the exact Blowfish-style bit ops; values must round-trip
// through the dynamic get/set path bit-for-bit.
const s = new Int32Array(1024);
for (let k = 0; k < 1024; k++) set(s, k, (k * 2654435761) | 0);
let acc = 0;
for (let k = 0; k < 1024; k++) acc = (acc ^ (get(s, k) >>> 24)) | 0;
console.log("A=" + acc);

// (B) out-of-bounds and negative reads → undefined; OOB write is dropped.
console.log("B=" + (get(s, 1024) === undefined) + "," + (get(s, -1) === undefined));
set(s, 5000, 123); // dropped, must not crash or grow
console.log("B2=" + (get(s, 5000) === undefined) + "," + get(s, 0));

// (C) per-kind coercion through the untyped store path.
const u8 = new Uint8Array(4);
set(u8, 0, 257);            // wraps to 1
set(u8, 1, -1);            // wraps to 255
const clamp = new Uint8ClampedArray(2);
set(clamp, 0, 300);        // clamps to 255
set(clamp, 1, -5);         // clamps to 0
const f64a = new Float64Array(2);
set(f64a, 0, 3.5);
console.log("C=" + get(u8, 0) + "," + get(u8, 1) + "," + get(clamp, 0) + "," + get(clamp, 1) + "," + get(f64a, 0));

// (D) exotic keys must still resolve as ordinary [[Get]]/[[Set]], NOT elements:
// `length` reads the real length, a string expando round-trips.
const anyU8: any = u8;
anyU8["foo"] = 42;
console.log("D=" + anyU8["length"] + "," + anyU8["foo"]);

// (E) BigInt kinds (deferred to the full dispatcher) still round-trip a bigint.
const big = new BigInt64Array(2);
set(big, 0, 9007199254740993n);
const bv: any = get(big, 0);
console.log("E=" + (typeof bv) + "," + bv);

// (F) an ArrayBuffer-backed offset view must still alias the buffer through the
// dynamic path (the fast path defers buffer-backed views to the slow path).
const buf = new ArrayBuffer(16);
const whole = new Int32Array(buf);
const view = new Int32Array(buf, 4, 2); // elements 1..2 of `whole`
set(view, 0, 777);
console.log("F=" + get(whole, 1) + "," + get(view, 0));
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
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        stdout,
        "A=116\n\
         B=true,true\n\
         B2=true,0\n\
         C=1,255,255,0,3.5\n\
         D=4,42\n\
         E=bigint,9007199254740993\n\
         F=777,777\n",
        "typed-array element access through an untyped receiver must match spec \
         semantics across kinds, bounds, exotic keys, BigInt, and buffer views"
    );
}
