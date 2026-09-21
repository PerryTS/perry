//! #10925 -- a `SharedArrayBuffer` must not have its kind decided by the bytes
//! that happen to sit in front of it.
//!
//! A SAB was handed to JS as the address of a header-less `alloc_zeroed`
//! block, and several paths read `addr - 8` as a `GcHeader` for it. The bytes
//! there are, in the allocator layout observed on Linux x86_64, the tail of
//! the PREVIOUS SAB's data -- user-writable through an ordinary typed-array
//! view. So writing a byte into one SAB's own memory changed `Array.isArray`
//! on another, and made `Map.prototype.get.call` on it dereference fabricated
//! pointers (SIGSEGV). A type confusion driven by user bytes.
//!
//! MUST-FAIL: committed BEFORE the fix. On the unfixed runtime the first test
//! prints `true` and the second segfaults (the harness reports the signal);
//! the expected strings are node 26.8.1's.

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
        "compiled binary failed (a signal here is #10925's segfault)\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// Wrong value, no crash: kind byte 1 (`GC_TYPE_ARRAY`) planted in the tail of
/// the first SAB made the second answer `Array.isArray(b) === true`.
#[test]
fn a_byte_written_into_one_sab_does_not_change_another_sabs_kind() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const sabs: any[] = [];
for (let i = 0; i < 8; i++) sabs.push(new SharedArrayBuffer(24));
for (const s of sabs) new Uint8Array(s)[16] = 1;
console.log("isArray", sabs.map((s) => Array.isArray(s)).join(","));
console.log("brand", Object.prototype.toString.call(sabs[3]));
console.log("json", JSON.stringify(sabs[3]));
"#,
    );
    assert_eq!(
        stdout,
        "isArray false,false,false,false,false,false,false,false\n\
         brand [object SharedArrayBuffer]\n\
         json {}\n"
    );
}

/// The segfault: kind byte 7 (`GC_TYPE_ERROR`) routed a collection thunk's
/// incompatible-receiver message into `js_error_get_name` on fabricated
/// pointers. node throws a `TypeError` for every one of these receivers.
#[test]
fn a_collection_brand_check_on_a_sab_throws_instead_of_crashing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const sabs: any[] = [];
for (let i = 0; i < 8; i++) sabs.push(new SharedArrayBuffer(24));
for (const s of sabs) { const u = new Uint8Array(s); u[16] = 7; u[20] = 64; }
let threw = 0;
for (const s of sabs) {
  try { Map.prototype.get.call(s, 1); } catch (e: any) { if (e instanceof TypeError) threw++; }
}
console.log("threw", threw);
console.log("keys", sabs.map((s) => Object.keys(s).length).join(","));
"#,
    );
    // Deliberately NOT asserting `String(sab)`: it returns the buffer bytes
    // (not `[object SharedArrayBuffer]`) for a plain `new ArrayBuffer(n)` too,
    // so that divergence is not a header read and would keep this test red
    // after the fix for a reason it does not name. Tracked separately.
    assert_eq!(
        stdout,
        "threw 8\n\
         keys 0,0,0,0,0,0,0,0\n"
    );
}

/// The sharing semantics the fix must NOT regress: two views over one SAB see
/// each other's writes, and so does a worker the SAB is handed to -- both by
/// closure capture and as a module-level binding (the escape hatch in
/// `closure_analysis.rs` that reads a top-level SAB in place from a worker).
#[test]
fn sab_bytes_are_shared_across_views_and_threads() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import { spawn } from "perry/thread";
const top = new SharedArrayBuffer(16);
const topView = new Int32Array(top);
topView[0] = 7;
async function main() {
  const local = new SharedArrayBuffer(16);
  const a = new Int32Array(local);
  const b = new Uint8Array(local);
  a[0] = 0x01020304;
  console.log("views", b[0], b[3]);
  const fromCapture = await spawn(() => {
    const v = new Int32Array(local);
    Atomics.add(v, 1, 5);
    return Atomics.load(v, 0);
  });
  console.log("capture", fromCapture, Atomics.load(a, 1));
  const fromTop = await spawn(() => {
    const v = new Int32Array(top);
    Atomics.store(v, 1, 99);
    return Atomics.load(v, 0);
  });
  console.log("module-level", fromTop, Atomics.load(topView, 1));
}
main();
"#,
    );
    assert_eq!(
        stdout,
        "views 4 1\n\
         capture 16909060 5\n\
         module-level 7 99\n"
    );
}
