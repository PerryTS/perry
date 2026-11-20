//! bun:ffi: a native `double`/`float` whose bits equal a NaN-box tag pattern
//! must reach JS as NaN, never as the value that tag spells (an int32, an
//! `undefined`, a heap pointer). Covers call returns, JSCallback arguments
//! (C -> JS) and `read.f64`/`read.f32`. node has no bun:ffi and the returns
//! need a native library, so this is a cargo harness, not a parity case.

use std::path::{Path, PathBuf};
use std::process::Command;

const LIB_C: &str = r#"
#include <stdint.h>
#include <string.h>
#if defined(_WIN32)
#define EXPORT __declspec(dllexport)
#else
#define EXPORT __attribute__((visibility("default")))
#endif
static double f64_of(uint32_t hi, uint32_t lo) {
    uint64_t b = ((uint64_t)hi << 32) | lo; double d; memcpy(&d, &b, 8); return d;
}
static float f32_of(uint32_t bits) { float f; memcpy(&f, &bits, 4); return f; }
EXPORT double nanb_f64(uint32_t hi, uint32_t lo) { return f64_of(hi, lo); }
EXPORT float nanb_f32(uint32_t bits) { return f32_of(bits); }
EXPORT void nanb_cb64(void (*cb)(double), uint32_t hi, uint32_t lo) { cb(f64_of(hi, lo)); }
EXPORT void nanb_cb32(void (*cb)(float), uint32_t bits) { cb(f32_of(bits)); }
"#;

const MAIN_TS: &str = r#"
import { dlopen, FFIType, JSCallback, ptr, read } from "bun:ffi";

const lib = dlopen(process.env.NANB_LIB!, {
  nanb_f64: { args: [FFIType.u32, FFIType.u32], returns: FFIType.f64 },
  nanb_f32: { args: [FFIType.u32], returns: FFIType.f32 },
  nanb_cb64: { args: [FFIType.function, FFIType.u32, FFIType.u32], returns: FFIType.void },
  nanb_cb32: { args: [FFIType.function, FFIType.u32], returns: FFIType.void },
});
const s = lib.symbols;

function show(label: string, x: any): void {
  console.log(label, typeof x, x === 5, Number.isNaN(x), Object.is(x, NaN), String(x));
}

let seen: any = 0;
const cb64 = new JSCallback((x: number) => { seen = x; }, { args: [FFIType.f64], returns: FFIType.void });
const cb32 = new JSCallback((x: number) => { seen = x; }, { args: [FFIType.f32], returns: FFIType.void });

// [hi, lo] u32 halves of the f64 pattern; heap-forging patterns last.
const F64: Array<[string, number, number]> = [
  ["quiet-payload", 0x7ff80000, 0x00000001],
  ["int32-5", 0x7ffe0000, 0x00000005],
  ["undefined", 0x7ffc0000, 0x00000001],
  ["null", 0x7ffc0000, 0x00000002],
  ["true", 0x7ffc0000, 0x00000004],
  ["neg-payload", 0xfffe0000, 0x00000005],
  ["handle", 0xfff90000, 0x00000001],
  ["pointer", 0x7ffd0000, 0x00000000],
  ["string", 0x7fff0000, 0x00000000],
];
// f32 NaN payloads widen to the f64 top halves 0x7FFE, 0x7FFC, 0xFFF9, 0x7FFD, 0x7FFF.
const F32: Array<[string, number]> = [
  ["f32-int32", 0x7ff00005],
  ["f32-undefined-band", 0x7fe00000],
  ["f32-handle", 0xffc80000],
  ["f32-pointer", 0x7fe80000],
  ["f32-string", 0x7ff80000],
];

for (const [name, hi, lo] of F64) {
  show("return " + name, s.nanb_f64(hi, lo));
  seen = 0;
  s.nanb_cb64(cb64.ptr, hi, lo);
  show("callback-arg " + name, seen);
  const buf = Buffer.alloc(8);
  buf.writeUInt32LE(lo, 0);
  buf.writeUInt32LE(hi, 4);
  show("read.f64 " + name, read.f64(ptr(buf), 0));
}
for (const [name, bits] of F32) {
  show("return " + name, s.nanb_f32(bits));
  seen = 0;
  s.nanb_cb32(cb32.ptr, bits);
  show("callback-arg " + name, seen);
  const buf = Buffer.alloc(4);
  buf.writeUInt32LE(bits, 0);
  show("read.f32 " + name, read.f32(ptr(buf), 0));
}
console.log("done");
"#;

fn build_lib(dir: &Path) -> PathBuf {
    let c_path = dir.join("nanb.c");
    std::fs::write(&c_path, LIB_C).expect("write C fixture");
    let lib = dir.join(if cfg!(target_os = "macos") {
        "libnanb.dylib"
    } else {
        "libnanb.so"
    });
    let status = Command::new("cc")
        .args(["-shared", "-fPIC", "-o"])
        .arg(&lib)
        .arg(&c_path)
        .status()
        .expect("run cc");
    assert!(status.success(), "cc failed to build the fixture library");
    lib
}

#[cfg(unix)]
#[test]
fn native_float_nan_payloads_read_as_nan() {
    let dir = tempfile::tempdir().expect("tempdir");
    let lib = build_lib(dir.path());
    let entry = dir.path().join("main.ts");
    std::fs::write(&entry, MAIN_TS).expect("write entry");
    let bin = dir.path().join("main_bin");
    let compile = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&bin)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&bin)
        .env("NANB_LIB", &lib)
        .output()
        .expect("run binary");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "binary failed: {}\nstdout:\n{stdout}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stderr)
    );
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.last(), Some(&"done"), "stdout:\n{stdout}");
    // 9 f64 patterns x 3 ingress paths + 5 f32 patterns x 3 + "done".
    assert_eq!(lines.len(), 9 * 3 + 5 * 3 + 1, "stdout:\n{stdout}");
    for line in &lines[..lines.len() - 1] {
        assert!(
            line.ends_with(" number false true true NaN"),
            "a native NaN payload escaped as a non-NaN value: {line}\nstdout:\n{stdout}"
        );
    }
}
