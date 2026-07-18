//! bun:ffi stage 1 (#6562) — e2e: compile TS that dlopens real C-ABI
//! dylibs and drive them through the typed call stubs.
//!
//! Two tiers:
//! 1. A purpose-built C dylib (compiled here with the system `cc`, the
//!    same toolchain the perry driver links with) exercising every
//!    stage-1 FFIType: integer widths/signs, i64/u64 → BigInt, f32/f64,
//!    bool, mixed int/float register assignment (the classic ABI trap),
//!    8-int / 8-double register limits, ptr round-trips through pinned
//!    Buffers (JS→native reads AND native→JS writes), cstring in both
//!    directions, NULL pointers, and the error surfaces (missing symbol,
//!    bad library, stage-1 rejections).
//! 2. A bun-pty smoke test against the real third-party
//!    `librust_pty` dylib (17-symbol pty FFI table): spawn a shell,
//!    write/read round-trip, resize, kill. Runs when the dylib is
//!    available (`BUN_PTY_LIB` env or a fresh `npm pack bun-pty@0.4.10`);
//!    skips with a note otherwise so offline CI stays green.

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &Path, entry: &Path, envs: &[(&str, &str)]) -> (bool, String, String) {
    let output = dir.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(entry)
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
    let mut run = Command::new(&output);
    for (k, v) in envs {
        run.env(k, v);
    }
    let run = run.output().expect("run compiled binary");
    (
        run.status.success(),
        String::from_utf8_lossy(&run.stdout).to_string(),
        String::from_utf8_lossy(&run.stderr).to_string(),
    )
}

/// Compile the tier-1 C fixture into a shared library with the system cc.
fn build_test_dylib(dir: &Path) -> PathBuf {
    let c_path = dir.join("ffi_test_lib.c");
    std::fs::write(&c_path, TEST_LIB_C).expect("write C fixture");
    let lib_name = if cfg!(target_os = "macos") {
        "libffi_test.dylib"
    } else {
        "libffi_test.so"
    };
    let lib_path = dir.join(lib_name);
    let status = Command::new("cc")
        .arg("-shared")
        .arg("-fPIC")
        .arg("-o")
        .arg(&lib_path)
        .arg(&c_path)
        .status()
        .expect("run cc (the perry link driver requires it too)");
    assert!(status.success(), "cc failed to build the test dylib");
    lib_path
}

// ─── tier 1: every FFIType against the purpose-built dylib ──────────────────

const TEST_LIB_C: &str = r#"
#include <stdint.h>
#include <stddef.h>
#include <string.h>
#include <stdbool.h>

#define EXPORT __attribute__((visibility("default")))

static int g_void_calls = 0;
EXPORT void ffi_void_bump(void) { g_void_calls++; }
EXPORT int32_t ffi_void_calls(void) { return g_void_calls; }

EXPORT bool ffi_not(bool v) { return !v; }
EXPORT bool ffi_is_forty_two(int32_t v) { return v == 42; }

/* Unsigned internal arithmetic so the wraparound at the type max is
 * well-defined (signed overflow is C UB — e.g. INT32_MAX + 1). The cast
 * back to the signed type is the implementation-defined 2's-complement
 * reinterpretation every real target uses, which is exactly the
 * wraparound the JS side asserts. */
EXPORT int8_t  ffi_i8_add1(int8_t v)  { return (int8_t)((uint8_t)v + 1u); }
EXPORT int16_t ffi_i16_add1(int16_t v) { return (int16_t)((uint16_t)v + 1u); }
EXPORT int32_t ffi_i32_add1(int32_t v) { return (int32_t)((uint32_t)v + 1u); }
EXPORT int64_t ffi_i64_add1(int64_t v) { return (int64_t)((uint64_t)v + 1u); }
EXPORT uint8_t  ffi_u8_add1(uint8_t v)  { return (uint8_t)(v + 1); }
EXPORT uint16_t ffi_u16_add1(uint16_t v) { return (uint16_t)(v + 1); }
EXPORT uint32_t ffi_u32_add1(uint32_t v) { return v + 1; }
EXPORT uint64_t ffi_u64_add1(uint64_t v) { return v + 1; }
EXPORT size_t   ffi_usize_add1(size_t v) { return v + 1; }

EXPORT int64_t  ffi_i64_min(void) { return INT64_MIN; }
EXPORT int64_t  ffi_i64_max(void) { return INT64_MAX; }
EXPORT uint64_t ffi_u64_max(void) { return UINT64_MAX; }

EXPORT float  ffi_f32_half(float v)  { return v * 0.5f; }
EXPORT double ffi_f64_half(double v) { return v * 0.5; }

EXPORT double ffi_mixed(int32_t a, double b, int32_t c, double d,
                        int64_t e, float f) {
    return (double)a + b * 2.0 + (double)c * 3.0 + d * 4.0 +
           (double)e * 5.0 + (double)f * 6.0;
}
EXPORT int64_t ffi_sum8(int32_t a, int32_t b, int32_t c, int32_t d,
                        int32_t e, int32_t f, int32_t g, int32_t h) {
    return (int64_t)a + b + c + d + e + f + g + h;
}
EXPORT double ffi_dsum8(double a, double b, double c, double d,
                        double e, double f, double g, double h) {
    return a + b + c + d + e + f + g + h;
}

EXPORT void *ffi_ptr_identity(void *p) { return p; }
EXPORT void *ffi_null_ptr(void) { return NULL; }
EXPORT uint8_t ffi_read_u8(const uint8_t *p, int32_t off) { return p[off]; }
EXPORT void ffi_fill(uint8_t *p, int32_t len, uint8_t value) {
    memset(p, value, (size_t)len);
}
EXPORT int64_t ffi_sum_bytes(const uint8_t *p, int32_t len) {
    int64_t acc = 0;
    for (int32_t i = 0; i < len; i++) acc += p[i];
    return acc;
}

EXPORT const char *ffi_hello(void) { return "hello from C"; }
EXPORT const char *ffi_empty_string(void) { return ""; }
EXPORT const char *ffi_null_string(void) { return NULL; }
EXPORT int32_t ffi_strlen(const char *s) { return (int32_t)strlen(s); }
static char g_concat_buf[256];
EXPORT const char *ffi_concat(const char *a, const char *b) {
    size_t la = strlen(a);
    size_t lb = strlen(b);
    if (la + lb + 1 > sizeof(g_concat_buf)) return "overflow";
    memcpy(g_concat_buf, a, la);
    memcpy(g_concat_buf + la, b, lb + 1);
    return g_concat_buf;
}
EXPORT const char *ffi_utf8(void) { return "caf\xC3\xA9 \xE2\x9C\x93"; }
"#;

const TIER1_TS: &str = r#"
import { dlopen, FFIType, ptr, CString, suffix } from "bun:ffi";

console.log("suffix-ok:", suffix === "dylib" || suffix === "so");
console.log("ffitype:", FFIType.i32, FFIType.cstring, FFIType.ptr, FFIType.void, FFIType.u64);
console.log("ffitype-aliases:", FFIType.pointer === FFIType.ptr, FFIType["int32_t"] === FFIType.i32, FFIType.usize === FFIType.u64);

const lib = dlopen(process.env.FFI_TEST_LIB!, {
  ffi_void_bump: { args: [], returns: FFIType.void },
  ffi_void_calls: { args: [], returns: FFIType.i32 },
  ffi_not: { args: [FFIType.bool], returns: FFIType.bool },
  ffi_is_forty_two: { args: [FFIType.i32], returns: FFIType.bool },
  ffi_i8_add1: { args: [FFIType.i8], returns: FFIType.i8 },
  ffi_i16_add1: { args: [FFIType.i16], returns: FFIType.i16 },
  ffi_i32_add1: { args: [FFIType.i32], returns: FFIType.i32 },
  ffi_i64_add1: { args: [FFIType.i64], returns: FFIType.i64 },
  ffi_u8_add1: { args: [FFIType.u8], returns: FFIType.u8 },
  ffi_u16_add1: { args: [FFIType.u16], returns: FFIType.u16 },
  ffi_u32_add1: { args: [FFIType.u32], returns: FFIType.u32 },
  ffi_u64_add1: { args: [FFIType.u64], returns: FFIType.u64 },
  ffi_usize_add1: { args: [FFIType.usize], returns: FFIType.usize },
  ffi_i64_min: { args: [], returns: FFIType.i64 },
  ffi_i64_max: { args: [], returns: FFIType.i64 },
  ffi_u64_max: { args: [], returns: FFIType.u64 },
  ffi_f32_half: { args: [FFIType.f32], returns: FFIType.f32 },
  ffi_f64_half: { args: [FFIType.f64], returns: FFIType.f64 },
  ffi_mixed: {
    args: [FFIType.i32, FFIType.f64, FFIType.i32, FFIType.f64, FFIType.i64, FFIType.f32],
    returns: FFIType.f64,
  },
  ffi_sum8: {
    args: [FFIType.i32, FFIType.i32, FFIType.i32, FFIType.i32, FFIType.i32, FFIType.i32, FFIType.i32, FFIType.i32],
    returns: FFIType.i64,
  },
  ffi_dsum8: {
    args: [FFIType.f64, FFIType.f64, FFIType.f64, FFIType.f64, FFIType.f64, FFIType.f64, FFIType.f64, FFIType.f64],
    returns: FFIType.f64,
  },
  ffi_ptr_identity: { args: [FFIType.ptr], returns: FFIType.ptr },
  ffi_null_ptr: { args: [], returns: FFIType.ptr },
  ffi_read_u8: { args: [FFIType.ptr, FFIType.i32], returns: FFIType.u8 },
  ffi_fill: { args: [FFIType.ptr, FFIType.i32, FFIType.u8], returns: FFIType.void },
  ffi_sum_bytes: { args: [FFIType.ptr, FFIType.i32], returns: FFIType.i64 },
  ffi_hello: { args: [], returns: FFIType.cstring },
  ffi_empty_string: { args: [], returns: FFIType.cstring },
  ffi_null_string: { args: [], returns: FFIType.cstring },
  ffi_strlen: { args: [FFIType.cstring], returns: FFIType.i32 },
  ffi_concat: { args: [FFIType.cstring, FFIType.cstring], returns: FFIType.cstring },
  ffi_utf8: { args: [], returns: FFIType.ptr },
});
const s = lib.symbols;

// void + side effect
s.ffi_void_bump();
s.ffi_void_bump();
console.log("void-calls:", s.ffi_void_calls());

// bool
console.log("not-true:", s.ffi_not(true), "not-false:", s.ffi_not(false));
console.log("is42:", s.ffi_is_forty_two(42), s.ffi_is_forty_two(41));

// integer widths, both signs (incl. wrap at the declared width)
console.log("i8:", s.ffi_i8_add1(-2), s.ffi_i8_add1(127));
console.log("i16:", s.ffi_i16_add1(-2), s.ffi_i16_add1(32767));
console.log("i32:", s.ffi_i32_add1(-2), s.ffi_i32_add1(2147483647));
console.log("u8:", s.ffi_u8_add1(254), s.ffi_u8_add1(255));
console.log("u16:", s.ffi_u16_add1(65534), s.ffi_u16_add1(65535));
console.log("u32:", s.ffi_u32_add1(4294967294), s.ffi_u32_add1(4294967295));

// i64/u64: bigint in AND out (Bun semantics: always bigint for i64/u64)
console.log("i64:", s.ffi_i64_add1(41n), typeof s.ffi_i64_add1(1n));
console.log("i64-number-arg:", s.ffi_i64_add1(41));
console.log("i64-min:", s.ffi_i64_min());
console.log("i64-max:", s.ffi_i64_max());
console.log("u64-max:", s.ffi_u64_max());
console.log("u64-wrap:", s.ffi_u64_add1(18446744073709551615n));
console.log("usize:", s.ffi_usize_add1(7n));

// floats
console.log("f32:", s.ffi_f32_half(9), "f64:", s.ffi_f64_half(9));

// mixed int/float register assignment + 8-arg register limits
console.log("mixed:", s.ffi_mixed(10, 2.0, 20, 4.0, 30, 1.5));
console.log("sum8:", s.ffi_sum8(1, 2, 3, 4, 5, 6, 7, 8));
console.log("dsum8:", s.ffi_dsum8(0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5));

// pointers: JS buffer -> native (read) and native -> JS buffer (write)
const buf = new Uint8Array(16);
for (let i = 0; i < 16; i++) buf[i] = i + 1;
const p = ptr(buf);
console.log("ptr-type:", typeof p, p !== 0);
console.log("ptr-identity:", s.ffi_ptr_identity(p) === p);
console.log("ptr-offset:", ptr(buf, 4) === p + 4);
console.log("null-ptr:", s.ffi_null_ptr());
console.log("read-u8:", s.ffi_read_u8(p, 0), s.ffi_read_u8(p, 15));
console.log("sum-bytes:", s.ffi_sum_bytes(p, 16));
s.ffi_fill(p, 16, 9);
console.log("fill-visible:", buf[0], buf[15]);

// Buffer also works as a pointer arg directly (Bun accepts views for ptr)
const nodeBuf = Buffer.from([1, 2, 3, 4]);
console.log("buffer-arg:", s.ffi_sum_bytes(nodeBuf, 4));

// cstring: out (decode), in (encode via NUL-terminated buffer)
console.log("hello:", s.ffi_hello());
console.log("empty:", JSON.stringify(s.ffi_empty_string()));
console.log("null-cstring:", s.ffi_null_string());
console.log("strlen:", s.ffi_strlen(Buffer.from("hello world\0")));
console.log("concat:", s.ffi_concat(Buffer.from("foo\0"), Buffer.from("bar\0")));

// CString: read a NUL-terminated string from a raw pointer
const utf8Ptr = s.ffi_utf8();
console.log("cstring-read:", CString(utf8Ptr));

lib.close();

// use-after-close throws a descriptive error instead of crashing
let closedError = "";
try {
  s.ffi_i32_add1(1);
} catch (e: any) {
  closedError = String(e && e.message);
}
console.log("closed-throws:", closedError.includes("close()"));

console.log("TIER1-DONE");
"#;

#[test]
fn tier1_every_ffi_type_against_test_dylib() {
    let dir = tempfile::tempdir().expect("tempdir");
    let lib_path = build_test_dylib(dir.path());
    let entry = dir.path().join("main.ts");
    std::fs::write(&entry, TIER1_TS).expect("write entry");

    let (ok, stdout, stderr) = compile_and_run(
        dir.path(),
        &entry,
        &[("FFI_TEST_LIB", lib_path.to_str().unwrap())],
    );
    assert!(ok, "binary failed\nstdout:\n{stdout}\nstderr:\n{stderr}");
    for needle in [
        "suffix-ok: true",
        "ffitype: 5 14 12 13 8",
        "ffitype-aliases: true true true",
        "void-calls: 2",
        "not-true: false not-false: true",
        "is42: true false",
        "i8: -1 -128",
        "i16: -1 -32768",
        "i32: -1 -2147483648",
        "u8: 255 0",
        "u16: 65535 0",
        "u32: 4294967295 0",
        "i64: 42n bigint",
        "i64-number-arg: 42n",
        "i64-min: -9223372036854775808n",
        "i64-max: 9223372036854775807n",
        "u64-max: 18446744073709551615n",
        "u64-wrap: 0n",
        "usize: 8n",
        "f32: 4.5 f64: 4.5",
        // 10 + 2*2 + 20*3 + 4*4 + 30*5 + 1.5*6 = 249
        "mixed: 249",
        "sum8: 36n",
        "dsum8: 32",
        "ptr-type: number true",
        "ptr-identity: true",
        "ptr-offset: true",
        "null-ptr: null",
        "read-u8: 1 16",
        "sum-bytes: 136n",
        "fill-visible: 9 9",
        "buffer-arg: 10n",
        "hello: hello from C",
        "empty: \"\"",
        "null-cstring: null",
        "strlen: 11",
        "concat: foobar",
        "cstring-read: caf\u{e9} \u{2713}",
        "closed-throws: true",
        "TIER1-DONE",
    ] {
        assert!(
            stdout.contains(needle),
            "expected `{needle}` in output:\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
}

// ─── tier 1b: error surfaces ────────────────────────────────────────────────

const ERRORS_TS: &str = r#"
import { dlopen, FFIType, ptr, JSCallback } from "bun:ffi";

// bad library path -> ERR_DLOPEN_FAILED with the path in the message
try {
  dlopen("/definitely/not/a/library.dylib", { f: { args: [], returns: FFIType.void } });
  console.log("open-missing: no-throw");
} catch (e: any) {
  console.log("open-missing:", String(e.message).includes("/definitely/not/a/library.dylib"));
}

// missing symbol -> names the symbol and the library
try {
  dlopen(process.env.FFI_TEST_LIB!, { not_a_real_symbol: { args: [], returns: FFIType.i32 } });
  console.log("missing-symbol: no-throw");
} catch (e: any) {
  console.log("missing-symbol:", String(e.message).includes('Symbol "not_a_real_symbol" not found'));
}

// FFIType.function -> clear stage-1 rejection at dlopen time
try {
  dlopen(process.env.FFI_TEST_LIB!, { ffi_hello: { args: [FFIType.function], returns: FFIType.void } });
  console.log("function-type: no-throw");
} catch (e: any) {
  console.log("function-type:", String(e.message).includes("not yet supported"));
}

// JSCallback export exists but throws the stage-1 error when used
try {
  JSCallback(() => {}, {});
  console.log("jscallback: no-throw");
} catch (e: any) {
  console.log("jscallback:", String(e.message).includes("not supported yet"));
}

// strings are not pointers (Bun-compatible hint)
try {
  ptr("hello" as any);
  console.log("string-ptr: no-throw");
} catch (e: any) {
  console.log("string-ptr:", String(e.message).includes("encode it as a buffer"));
}

console.log("ERRORS-DONE");
"#;

#[test]
fn tier1b_error_surfaces() {
    let dir = tempfile::tempdir().expect("tempdir");
    let lib_path = build_test_dylib(dir.path());
    let entry = dir.path().join("main.ts");
    std::fs::write(&entry, ERRORS_TS).expect("write entry");

    let (ok, stdout, stderr) = compile_and_run(
        dir.path(),
        &entry,
        &[("FFI_TEST_LIB", lib_path.to_str().unwrap())],
    );
    assert!(ok, "binary failed\nstdout:\n{stdout}\nstderr:\n{stderr}");
    for needle in [
        "open-missing: true",
        "missing-symbol: true",
        "function-type: true",
        "jscallback: true",
        "string-ptr: true",
        "ERRORS-DONE",
    ] {
        assert!(
            stdout.contains(needle),
            "expected `{needle}` in output:\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
}

// ─── tier 2: bun-pty smoke against the real third-party dylib ───────────────

/// Locate (or fetch) bun-pty 0.4.10's prebuilt dylib for this platform.
/// Resolution order: `BUN_PTY_LIB` env → `npm pack bun-pty@0.4.10` into the
/// test tempdir. Returns `None` (→ skip, with a note) when neither works.
fn locate_bun_pty_dylib(dir: &Path) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("BUN_PTY_LIB") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }
    let pack = Command::new("npm")
        .current_dir(dir)
        .args(["pack", "bun-pty@0.4.10", "--silent"])
        .output()
        .ok()?;
    if !pack.status.success() {
        return None;
    }
    let tgz = dir.join("bun-pty-0.4.10.tgz");
    if !tgz.exists() {
        return None;
    }
    let untar = Command::new("tar")
        .current_dir(dir)
        .args(["xzf", "bun-pty-0.4.10.tgz"])
        .status()
        .ok()?;
    if !untar.success() {
        return None;
    }
    let release = dir.join("package/rust-pty/target/release");
    let name = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "librust_pty_arm64.dylib",
        ("macos", _) => "librust_pty.dylib",
        ("linux", "aarch64") => "librust_pty_arm64.so",
        ("linux", _) => "librust_pty.so",
        _ => return None,
    };
    let p = release.join(name);
    p.exists().then_some(p)
}

/// Mirrors the FFI usage of bun-pty's `src/terminal.ts` (same symbols, same
/// signatures) without the package's EventEmitter scaffolding: spawn a real
/// shell through the prebuilt Rust dylib, round-trip bytes through the pty,
/// resize, then kill.
const BUN_PTY_TS: &str = r#"
import { dlopen, FFIType, ptr } from "bun:ffi";

const lib = dlopen(process.env.BUN_PTY_LIB!, {
  bun_pty_spawn: {
    args: [FFIType.cstring, FFIType.cstring, FFIType.cstring, FFIType.i32, FFIType.i32],
    returns: FFIType.i32,
  },
  bun_pty_write: { args: [FFIType.i32, FFIType.pointer, FFIType.i32], returns: FFIType.i32 },
  bun_pty_read: { args: [FFIType.i32, FFIType.pointer, FFIType.i32], returns: FFIType.i32 },
  bun_pty_resize: { args: [FFIType.i32, FFIType.i32, FFIType.i32], returns: FFIType.i32 },
  bun_pty_kill: { args: [FFIType.i32], returns: FFIType.i32 },
  bun_pty_get_pid: { args: [FFIType.i32], returns: FFIType.i32 },
  bun_pty_get_exit_code: { args: [FFIType.i32], returns: FFIType.i32 },
  bun_pty_close: { args: [FFIType.i32], returns: FFIType.void },
});
const s = lib.symbols;

// Spawn `sh` exactly the way bun-pty's Terminal ctor does (shell-quoted
// cmdline, cwd, NUL-separated env, cols, rows).
const handle = s.bun_pty_spawn(
  Buffer.from("'sh'\0", "utf8"),
  Buffer.from(process.cwd() + "\0", "utf8"),
  Buffer.from("PATH=/usr/bin:/bin\0TERM=xterm\0\0", "utf8"),
  80,
  24,
);
console.log("spawned:", handle >= 0);
const pid = s.bun_pty_get_pid(handle);
console.log("pid-positive:", pid > 0);

// write/read round-trip: the pty has terminal echo ON, so the INPUT line is
// echoed back verbatim. To prove the SHELL actually ran (not just that our
// keystrokes bounced), send a command the shell must EVALUATE — arithmetic
// expansion — and assert on its *result*. The input echo shows the literal
// `echo FFI_$((40+2))_OK`, which does not contain `FFI_42_OK`; only the
// shell's computed output does.
const expected = "FFI_42_OK";
const cmd = Buffer.from("echo FFI_$((40+2))_OK\n", "utf8");
s.bun_pty_write(handle, ptr(cmd), cmd.length);

const readBuf = Buffer.allocUnsafe(4096);
let collected = "";
const deadline = Date.now() + 10000;
while (Date.now() < deadline) {
  const n = s.bun_pty_read(handle, ptr(readBuf), readBuf.length);
  if (n > 0) {
    collected += readBuf.subarray(0, n).toString("utf8");
    if (collected.includes(expected)) break;
  } else if (n === -2) {
    break; // child exited
  } else if (n < 0) {
    break;
  } else {
    // 0 bytes: brief spin-wait (no timers needed for the smoke test)
    const until = Date.now() + 8;
    while (Date.now() < until) {}
  }
}
console.log("roundtrip:", collected.includes(expected));

console.log("resize:", s.bun_pty_resize(handle, 120, 40) === 0);

s.bun_pty_kill(handle);
s.bun_pty_close(handle);
console.log("killed: true");

lib.close();
console.log("PTY-DONE");
"#;

#[test]
fn tier2_bun_pty_shell_roundtrip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let Some(dylib) = locate_bun_pty_dylib(dir.path()) else {
        eprintln!(
            "SKIP tier2_bun_pty_shell_roundtrip: bun-pty dylib unavailable \
             (set BUN_PTY_LIB or allow `npm pack bun-pty@0.4.10`)"
        );
        return;
    };
    let entry = dir.path().join("main.ts");
    std::fs::write(&entry, BUN_PTY_TS).expect("write entry");

    let (ok, stdout, stderr) = compile_and_run(
        dir.path(),
        &entry,
        &[("BUN_PTY_LIB", dylib.to_str().unwrap())],
    );
    assert!(ok, "binary failed\nstdout:\n{stdout}\nstderr:\n{stderr}");
    for needle in [
        "spawned: true",
        "pid-positive: true",
        "roundtrip: true",
        "resize: true",
        "killed: true",
        "PTY-DONE",
    ] {
        assert!(
            stdout.contains(needle),
            "expected `{needle}` in output:\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
}
