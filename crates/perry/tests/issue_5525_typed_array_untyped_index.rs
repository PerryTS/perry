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

/// #5525 follow-up: closing the bcrypt perf gap relaxed the unknown-receiver
/// index gates so that *all* non-static-string/symbol element accesses on an
/// `any` receiver route through `js_dyn_index_get` / `js_dyn_index_set` (which
/// carry the cached typed-array fast path) — including the `lr[off]` /
/// `lr[off + 1]` writes whose index is not statically numeric. That widening
/// must not change semantics for the non-typed-array cases the same dispatchers
/// now also serve: runtime string keys, numeric-string keys, Symbol keys, plain
/// arrays, and plain objects. It also adds a both-operands-plain-number fast
/// path to the dynamic `+`; this pins that `+` still concatenates strings and
/// coerces mixed operands per spec.
#[test]
fn untyped_index_routing_preserves_non_typed_array_semantics() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");

    std::fs::write(
        &entry,
        r#"
// All accesses go through untyped params, so the access site can't narrow the
// receiver and must route through the dynamic get/set dispatchers.
function g(a: any, k: any): any { return a[k]; }
function s(a: any, k: any, v: any): void { a[k] = v; }

// (A) typed array with a NON-statically-numeric index (mirrors bcryptjs
// `lr[off]` / `lr[off + 1]` where `off` is an `any` param).
const ta = new Int32Array(4);
const off: any = 0;
s(ta, off, 100);
s(ta, off + 1, -7);
console.log("A=" + g(ta, off) + "," + g(ta, off + 1) + "," + (g(ta, 9) === undefined));

// (B) plain object reached through the untyped path: a runtime string key and a
// numeric-string key must land as ordinary properties, not elements.
const o: any = {};
const key: any = "fo" + "o"; // runtime (non-literal) string
s(o, key, 42);
s(o, "7", 9);
console.log("B=" + g(o, key) + "," + o.foo + "," + g(o, "7"));

// (C) a Symbol key must resolve through the symbol side-table on both get & set.
const sym: any = Symbol("x");
const o2: any = {};
s(o2, sym, "viaSym");
console.log("C=" + g(o2, sym) + "," + (g(o2, "nope") === undefined));

// (D) a plain array grown through the untyped write path.
const arr: any = [];
s(arr, 0, 11);
s(arr, 1, 22);
console.log("D=" + arr[0] + "," + arr[1] + "," + arr.length);

// (E) the dynamic `+` plain-number fast path must not change string concat or
// mixed-operand coercion.
function add(a: any, b: any): any { return a + b; }
console.log("E=" + add(2, 3) + "," + add("x", 5) + "," + add(1, "y") + "," + add(2.5, 0.5));
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
        "A=100,-7,true\n\
         B=42,42,9\n\
         C=viaSym,true\n\
         D=11,22,2\n\
         E=5,x5,1y,3\n",
        "widening the unknown-receiver index routing must preserve spec semantics \
         for runtime-string / numeric-string / Symbol keys, plain arrays & objects, \
         and the dynamic `+` fast path"
    );
}

/// #5525 follow-up #2: the unknown-receiver element read/write is now lowered as
/// a guarded **inline** typed-array load/store at the access site (cache probe +
/// bounds check + direct slot access), with the runtime `js_dyn_index_{get,set}`
/// kept as the fall-back for any guard miss. This pins that the inline path is
/// behaviourally identical to the slow path across: every inlined element kind
/// (Int8…Float64, signed/unsigned/float), in-bounds round-trips, the OOB /
/// negative / fractional / NaN index deferrals, the kinds the inline guard
/// excludes (Uint8Clamped clamp, BigInt, Float16), and the `PERRY_TA_VIEW_GUARD`
/// case where a live ArrayBuffer-backed view bars the inline path so *all*
/// typed-array accesses (even on owning arrays) take the slow path yet stay
/// correct.
#[test]
fn inline_typed_array_fast_path_matches_slow_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");

    std::fs::write(
        &entry,
        r#"
function g(a: any, i: any): any { return a[i]; }
function s(a: any, i: any, v: any): void { a[i] = v; }

// (A) every inlined kind round-trips a representative value, incl. signed/
// unsigned narrowing wraps and float precision.
const i8 = new Int8Array(1);  s(i8, 0, 200);          // -> -56
const u8 = new Uint8Array(1); s(u8, 0, 300);          // -> 44
const i16 = new Int16Array(1); s(i16, 0, -1);
const u16 = new Uint16Array(1); s(u16, 0, 70000);     // -> 4464
const i32 = new Int32Array(1); s(i32, 0, 4294967297); // -> 1
const u32 = new Uint32Array(1); s(u32, 0, -1);        // -> 4294967295
const f32 = new Float32Array(1); s(f32, 0, 1.5);
const f64 = new Float64Array(1); s(f64, 0, 3.14159);
console.log("A=" + g(i8,0) + "," + g(u8,0) + "," + g(i16,0) + "," + g(u16,0) +
            "," + g(i32,0) + "," + g(u32,0) + "," + g(f32,0) + "," + g(f64,0));

// (B) index deferrals: OOB / negative / NaN must NOT take the inline load (the
// inline guard rejects them and defers to the runtime). The fractional case
// (`g(t,1.5)`) is included to prove the inline guard *defers* it to the slow
// path — but the assertion mirrors the runtime slow path's existing behaviour
// (a pre-existing quirk: the untyped dynamic getter truncates a fractional
// index rather than returning undefined, identical on clean origin/main),
// since the whole point is that inline == slow.
const t = new Int32Array(3); s(t, 0, 10); s(t, 1, 20); s(t, 2, 30);
console.log("B=" + (g(t,3) === undefined) + "," + (g(t,-1) === undefined) +
            "," + (g(t,1.5) === g(t,1)) + "," + (g(t,NaN) === undefined));

// (C) inline-excluded kinds defer correctly: Uint8Clamped clamps, BigInt boxes.
const c = new Uint8ClampedArray(2); s(c, 0, 300); s(c, 1, -5);
const b = new BigInt64Array(1); s(b, 0, 5n);
console.log("C=" + g(c,0) + "," + g(c,1) + "," + (typeof g(b,0)) + "," + g(b,0));

// (D) PERRY_TA_VIEW_GUARD: once an ArrayBuffer-backed view is live, the inline
// path must be barred for ALL typed arrays (owning included) and every access
// must still be correct via the slow path.
const own = new Int32Array(2); s(own, 0, 111); s(own, 1, 222);
const ab = new ArrayBuffer(8);
const v = new Int32Array(ab);           // bumps the view guard
s(v, 0, 333); s(v, 1, 444);
console.log("D=" + g(own,0) + "," + g(own,1) + "," + g(v,0) + "," + g(v,1));
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
        "A=-56,44,-1,4464,1,4294967295,1.5,3.14159\n\
         B=true,true,true,true\n\
         C=255,0,bigint,5\n\
         D=111,222,333,444\n",
        "the inline typed-array fast path must be bit-identical to the runtime \
         slow path across kinds, index deferrals, excluded kinds, and the \
         view-guard fallback"
    );
}
