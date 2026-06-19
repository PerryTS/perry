//! Regression test: arbitrary user-assigned OWN properties on a value the HIR
//! tagged as a native (classic Node stream) instance must READ/WRITE/INDEX as
//! plain object properties — while genuine native stream methods STILL
//! dispatch.
//!
//! Pre-fix wall (bundled `debug` package, pulled in by https-proxy-agent):
//! `debug`'s `selectColor` reads `_.colors` (an ARRAY field of the createDebug
//! value `_`). Because `_` was HIR-tagged a native instance, the catch-all
//! `else` arm in `crates/perry-hir/src/lower/expr_member.rs` lowered EVERY
//! non-method property read to a 0-arg `NativeMethodCall`, which codegen turned
//! into `js_native_call_method_nullsafe(inst, "colors", 0 args)` — i.e. it
//! *invoked* the stored array → `TypeError: value is not a function`. Node just
//! returns the property value.
//!
//! Fix: for `stream` / `node:stream` instances, a bare read of a name that is
//! neither a known classic stream method (`is_classic_stream_method_name`) nor
//! a known stream property getter (`is_classic_stream_getter_name`) lowers to a
//! plain `PropertyGet` (own-property storage on the heap stream object). The
//! matching `_.x = v` write already lowers to a generic `PropertySet`, so the
//! value persists and reads back. Real native methods (`.on`, `.pipe`,
//! `.destroy`, `.write`, `.end`) and getters (`.destroyed`) keep dispatching.

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
        "compiled binary failed (pre-fix: 'TypeError: value is not a \
         function' when reading an own property of a native stream \
         instance)\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn native_instance_own_property_read_write_index_and_method() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import { Readable } from "stream";

const _: any = Readable.from(["a"]);

// WRITE an arbitrary own property (an array), then READ it back. Pre-fix the
// READ invoked the array as a 0-arg native method → "value is not a function".
_.colors = ["red", "green", "blue"];
console.log("colors:", JSON.stringify(_.colors));
console.log("len:", _.colors.length);   // member read of the own array
console.log("idx:", _.colors[1]);       // index into the own array

// A scalar own property reads back its value (not invoked).
_.label = "stream-label";
console.log("label:", _.label);

// A genuine native stream method STILL dispatches in the same instance.
const got: number[] = [];
const r2: any = Readable.from([1, 2, 3]);
r2.on("data", (c: any) => { got.push(Number(c)); });
r2.on("end", () => {
  console.log("data:", got.join(","));
  console.log("destroyed-before:", r2.destroyed); // native getter still works
  r2.destroy();
  console.log("destroyed-after:", r2.destroyed);
});
"#,
    );

    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.contains(&"colors: [\"red\",\"green\",\"blue\"]"),
        "own array property must read back the stored value, got:\n{stdout}"
    );
    assert!(
        lines.contains(&"len: 3"),
        "member read on own array property must work, got:\n{stdout}"
    );
    assert!(
        lines.contains(&"idx: green"),
        "index into own array property must work, got:\n{stdout}"
    );
    assert!(
        lines.contains(&"label: stream-label"),
        "scalar own property must read back its value, got:\n{stdout}"
    );
    assert!(
        lines.contains(&"data: 1,2,3"),
        "native stream method (.on) must still dispatch, got:\n{stdout}"
    );
    assert!(
        lines.contains(&"destroyed-before: false") && lines.contains(&"destroyed-after: true"),
        "native stream getter (.destroyed) + method (.destroy) must still \
         dispatch, got:\n{stdout}"
    );
}

/// #wall (debug `_.colors`): the same own-property bug, but for the NON-`stream`
/// native-instance modules. `debug`'s `createDebug` value is HIR-tagged as one
/// of the opaque-handle native modules (`blob` / `fetch` / `readable_stream` /
/// …), so `selectColor`'s `_.colors[...]` READ lowered to a 0-arg
/// `js_native_call_method_nullsafe(_, "colors", 0 args)` that *invoked* the
/// resolved value → `TypeError: value is not a function`. The classic-`stream`
/// guard did NOT cover these modules.
///
/// Two-part fix:
///   1. HIR (`lower/expr_member.rs`): an arbitrary property READ on a `blob` /
///      `fetch` instance that is not a known native data getter now lowers to a
///      plain `PropertyGet` (READS the value instead of CALLING it).
///   2. Runtime (`object/handle_expando.rs` + the `js_handle_property_*_dispatch`
///      fallbacks): a generic per-handle expando side-table — keyed by handle
///      id, GC-traced like closures' `CLOSURE_PROPS` — so a WRITE of an arbitrary
///      own property persists and reads back, matching Node (these handles are
///      ordinary extensible objects). Typed getters (`blob.size`, `res.status`)
///      are consulted before the expando, so they always win.
///
/// This pins: arbitrary own-property WRITE → READ → member/index round-trips on
/// a `Blob` (`module=blob`) and a fetch `Response` (`module=fetch`), with the
/// genuine native data getters STILL dispatching through their FFI.
#[test]
fn native_instance_own_property_non_stream_modules() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
// ── Blob (module=blob, class=Blob) ──
const b: any = new Blob(["hello"]);
b.colors = [6, 2, 3, 4, 5, 1];        // arbitrary own array property
console.log("blob.colors:", JSON.stringify(b.colors));   // pre-fix: invoked the array
console.log("blob.colors.len:", b.colors.length);
console.log("blob.colors.idx:", b.colors[2]);
b.label = "blob-label";
console.log("blob.label:", b.label);
console.log("blob.size:", b.size);    // native getter must STILL dispatch

// ── fetch Response (module=fetch, class=Response) ──
const res: any = new Response("payload");
res.colors = ["r", "g", "b"];
console.log("res.colors:", JSON.stringify(res.colors));
console.log("res.colors.len:", res.colors.length);
console.log("res.status:", res.status);   // native getter must STILL dispatch
console.log("res.ok:", res.ok);
"#,
    );

    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.contains(&"blob.colors: [6,2,3,4,5,1]"),
        "Blob arbitrary own array property must read back stored value, got:\n{stdout}"
    );
    assert!(
        lines.contains(&"blob.colors.len: 6"),
        "member read on Blob own array property must work, got:\n{stdout}"
    );
    assert!(
        lines.contains(&"blob.colors.idx: 3"),
        "index into Blob own array property must work, got:\n{stdout}"
    );
    assert!(
        lines.contains(&"blob.label: blob-label"),
        "scalar own property on Blob must read back its value, got:\n{stdout}"
    );
    assert!(
        lines.contains(&"blob.size: 5"),
        "native Blob getter (.size) must STILL dispatch, got:\n{stdout}"
    );
    assert!(
        lines.contains(&"res.colors: [\"r\",\"g\",\"b\"]"),
        "Response arbitrary own property must read back stored value, got:\n{stdout}"
    );
    assert!(
        lines.contains(&"res.colors.len: 3"),
        "member read on Response own array property must work, got:\n{stdout}"
    );
    assert!(
        lines.contains(&"res.status: 200"),
        "native Response getter (.status) must STILL dispatch, got:\n{stdout}"
    );
    assert!(
        lines.contains(&"res.ok: true"),
        "native Response getter (.ok) must STILL dispatch, got:\n{stdout}"
    );
}
