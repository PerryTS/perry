//! Regression test for #6088: an out-of-bounds integer index of a typed array
//! returned `0` instead of `undefined`.
//!
//! `ta[i]` for a canonical numeric index outside `[0, length)` must read
//! `undefined` (ECMAScript IntegerIndexedExotic `[[Get]]`), NOT `0`. Most
//! typed-array kinds already took the correct runtime helper (`js_typed_array_get`,
//! OOB → `undefined`). But `Uint8Array` (and Node `Buffer`) are Buffer-backed in
//! perry and share a distinct codegen path: for a proven non-negative integer
//! key whose value was not proven in bounds, the fallback called the native
//! `js_buffer_get`, whose i32 return type forces a `0` byte-sentinel for
//! out-of-range reads — so `new Uint8Array([5,6,7])[9]` read `0`, and `typeof`
//! was `"number"`.
//!
//! Fix: route that unproven-bounds slow path through a new JS-value accessor
//! (`js_buffer_index_get_value`) that returns `undefined` for out-of-range while
//! still returning the byte as a number for in-range reads. Negative / fractional
//! keys already routed to the dynamic-key helper and stayed correct; the
//! proven-bounds inline load is untouched, so in-range reads keep the fast path.
//!
//! Expected outputs are byte-for-byte what `node --experimental-strip-types`
//! prints. The scan-loop case is timeout-bounded: pre-fix, a typed-array index
//! scan whose only loop exit is an OOB `undefined` spun forever (`0 !==
//! undefined`), so a regression FAILS (spins) instead of silently misreading.

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

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
        .arg("--no-cache")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let mut child = Command::new(&output)
        .current_dir(dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn compiled binary");

    let timeout = Duration::from_secs(30);
    let start = Instant::now();
    loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => {
                let out = child.wait_with_output().expect("wait_with_output");
                assert!(
                    status.success(),
                    "compiled binary failed (exit {:?})\nstdout:\n{}\nstderr:\n{}",
                    status.code(),
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
                return String::from_utf8_lossy(&out.stdout).into_owned();
            }
            None => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!(
                        "compiled binary did not finish within {:?} — a typed-array \
                         OOB index regressed (returns 0 not undefined)",
                        timeout
                    );
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

/// The core bug plus the neighbours that must stay correct: OOB integer index of
/// a Uint8Array / Buffer is `undefined` (not `0`), in-range reads are the byte,
/// negative / fractional keys are `undefined`, `any`-typed and variable indices
/// still reach the fix, and the already-correct kinds (Float64/Int32/Int8/Uint16)
/// are unaffected.
#[test]
fn typed_array_oob_index_is_undefined() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const u8 = new Uint8Array([5, 6, 7]);
console.log("u8 in", u8[0], u8[2], "oob", u8[9] === undefined, typeof u8[9]);
const k = 9;
console.log("u8 var-oob", u8[k] === undefined);
const anyU8: any = new Uint8Array([1, 2]);
console.log("u8 any-oob", anyU8[5] === undefined, typeof anyU8[5]);
const z = new Uint8Array(2);
console.log("u8 zero", z[0], z[1], "oob", z[5] === undefined);
console.log("u8 neg/frac", u8[-1] === undefined, u8[1.5] === undefined);
const f64 = new Float64Array([1.5, 2.5]);
const i32 = new Int32Array([10, 20]);
const i8 = new Int8Array([-1, -2]);
const u16 = new Uint16Array([300, 400]);
console.log("f64", f64[0], f64[9] === undefined);
console.log("i32", i32[1], i32[9] === undefined);
console.log("i8", i8[0], i8[9] === undefined);
console.log("u16", u16[1], u16[9] === undefined);
const buf = Buffer.from([65, 66, 67]);
console.log("buf", buf[0], "oob", buf[9] === undefined);
"#,
    );
    assert_eq!(
        stdout,
        "u8 in 5 7 oob true undefined\n\
         u8 var-oob true\n\
         u8 any-oob true undefined\n\
         u8 zero 0 0 oob true\n\
         u8 neg/frac true true\n\
         f64 1.5 true\n\
         i32 20 true\n\
         i8 -1 true\n\
         u16 400 true\n\
         buf 65 oob true\n"
    );
}

/// A consumer that scans a Uint8Array by index in a loop whose only exit is an
/// out-of-bounds read comparing `=== undefined` in the loop CONDITION (the
/// IntegerIndexedExotic `[[Get]]` value context this fix targets). Pre-fix the
/// OOB read was `0` (`0 !== undefined`), so the loop never terminated and spun
/// forever; the timeout bound turns a regression into a FAIL rather than a hang.
#[test]
fn typed_array_scan_loop_terminates_on_oob() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function scanU8(a: Uint8Array): number {
  const out: number[] = [];
  let i = 0;
  while (a[i] !== undefined) {
    out.push(a[i] as number);
    i++;
  }
  return out.length;
}
console.log("scan", scanU8(new Uint8Array([1, 2, 3, 4])));
"#,
    );
    assert_eq!(stdout, "scan 4\n");
}
