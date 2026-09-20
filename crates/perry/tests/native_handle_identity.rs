//! #10821 -- object identity for native-handle receivers.
//!
//! A native-module handle used to travel as a small registry integer under
//! `POINTER_TAG`, so a value's identity was its registry id rather than its
//! object identity. `TextEncoder` is the extreme case: it is stateless, so
//! every instance shared one sentinel id and *every* `TextEncoder` in a program
//! was literally the same value -- `a === b` was true, two of them collapsed
//! into one `Map` key, and a `WeakMap` entry stored under `a` was readable
//! through an unrelated `b`.
//!
//! With honest tags a handle is a real GC cell and identity is the cell
//! address, which is what all of these operations actually want.
//!
//! These tests are the identity contract every migrated family must satisfy:
//! two distinct resources are `!==`, the same resource reached twice is `===`,
//! and both hold as `Map` / `Set` / `WeakMap` keys.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
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

    let run = Command::new(&output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// Every row here was wrong before the change; the expected strings are node's
/// (node 26.8.1) output for the same program.
#[test]
fn distinct_text_handles_are_distinct_objects() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const a: any = new TextEncoder();
const b: any = new TextEncoder();
console.log(a === b, Object.is(a, b), a === a);
const m = new Map([[a, "one"], [b, "two"]]);
console.log(m.size, m.get(a), m.get(b));
console.log(new Set([a, b]).size);
const wm = new WeakMap();
wm.set(a, "A");
console.log(wm.get(b), wm.get(a));
console.log([a].indexOf(b), [a].indexOf(a));
const d1: any = new TextDecoder();
const d2: any = new TextDecoder();
console.log(d1 === d2, d1 === d1);
"#,
    );
    assert_eq!(
        stdout,
        "false false true\n\
         2 one two\n\
         2\n\
         undefined A\n\
         -1 0\n\
         false true\n"
    );
}

/// The other half: one resource reached through several routes is ONE object.
/// A family whose registry hands out a fresh cell per lookup would fail here.
#[test]
fn the_same_text_handle_reached_twice_is_the_same_object() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const d: any = new TextDecoder("latin1");
const holder: any = { d };
const viaField = holder.d;
const viaFn = ((x: any) => x)(d);
const viaArray = [d][0];
console.log(viaField === d, viaFn === d, viaArray === d);
const m = new Map();
m.set(d, "state");
console.log(m.get(viaField), m.get(viaFn), m.get(viaArray), m.size);
const wm = new WeakMap();
wm.set(d, "private");
console.log(wm.get(viaField));
// identity must survive a collection, and so must the registry entry
let sink = 0;
for (let i = 0; i < 200000; i++) { const o = { a: i }; sink += o.a; }
console.log(viaField === d, d.encoding, sink);
"#,
    );
    assert_eq!(
        stdout,
        "true true true\n\
         state state state 1\n\
         private\n\
         true windows-1252 19999900000\n"
    );
}

/// The honest representation must not disturb the surface the family already
/// had: #8133's `K.decode.bind(K)` shape, the accessors, and encode/decode.
#[test]
fn text_handle_surface_survives_the_representation_change() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const e: any = new TextEncoder();
const d: any = new TextDecoder();
console.log(typeof e, typeof d, typeof d.decode, typeof e.encode);
console.log(Object.prototype.toString.call(e), Object.prototype.toString.call(d));
console.log(e.encoding, d.encoding, d.fatal, d.ignoreBOM);
const bound = d.decode.bind(d);
console.log(bound(new Uint8Array([104, 105])), d.decode(new Uint8Array([111, 107])));
const u8 = e.encode("hi");
console.log(u8 instanceof Uint8Array, u8[0], u8[1]);
const dest = new Uint8Array(4);
const r = e.encodeInto("ab", dest);
console.log(r.read, r.written, dest[0], dest[1]);
"#,
    );
    assert_eq!(
        stdout,
        "object object function function\n\
         [object TextEncoder] [object TextDecoder]\n\
         utf-8 utf-8 false false\n\
         hi ok\n\
         true 104 105\n\
         2 2 97 98\n"
    );
}
