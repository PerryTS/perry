//! V8-interop no-op stubs and AOT no-op stubs for unconditionally-declared
//! FFI symbols (lodash, axios, argon2, sharp, ratelimit).

// V8 interop no-op stubs. Perry no longer ships a runtime JS engine: the
// `perry-jsruntime` crate (V8 via `deno_core`) that used to provide the real
// implementations of these symbols was deleted in #1696 ("remove
// perry-jsruntime; ship V8-free binaries"). These stubs are now the *only*
// definitions of `js_load_module`/`js_call_function`/etc. — every build links
// them, and `js_*` calls always return the "no JS runtime" sentinel below.
// (`enforce_js_runtime_gate` in
// `crates/perry/src/commands/compile/bootstrap.rs` hard-errors any build
// that would actually need one of these to do real work, so in practice a
// compiled binary never calls through to them for a live JS module — they
// exist only to satisfy the codegen-side `js_*` FFI declarations that are
// still unconditionally emitted.)
//
// Signatures must match `crates/perry-codegen/src/runtime_decls.rs` exactly — the codegen
// declarations determine which register the caller reads the result from (rax/x0 for I64,
// xmm0/d0 for DOUBLE). A signature mismatch reads garbage and silently miscompiles.
//
// Stubs return NaN-boxed `TAG_UNDEFINED` (not 0.0) so downstream `typeof`
// correctly observes `undefined` instead of `"number"` — making the
// no-JS-runtime case diagnostically distinct from a successful
// 0-returning JS call, in case any of these symbols is ever reached.
//
// On macOS (Mach-O) the stubs are still emitted as **weak** symbols via
// `global_asm!` (`.weak_definition`) — a holdover from when a strong
// definition in `libperry_jsruntime.a` needed to be able to override them
// regardless of linker archive scan order (Issue #257). With
// `perry-jsruntime` gone there is no strong definition left to win that
// race, so the weak attribute is currently a no-op; it's left in place
// rather than downgraded to a plain global so re-adding a real JS runtime
// wouldn't require rediscovering this mechanism.
//
// On other platforms (Linux, iOS, Android, Windows), the plain Rust
// functions below are the only definitions and behave the same way —
// unconditional no-ops.

const _UNDEF_BITS: u64 = crate::value::TAG_UNDEFINED;

// On Mach-O arm64, emit weak symbol stubs that return NaN-boxed TAG_UNDEFINED
// (0x7FFC_0000_0000_0001) for f64-returning FFIs, 0 for i64-returning,
// nothing for void. .weak_definition tells ld64 to treat this as a weak
// symbol so a strong def from libperry_jsruntime.a wins regardless of
// archive scan order.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
core::arch::global_asm!(
    // js_load_module(i64, i64) -> i64 ;  return 0 (handle 0 = invalid)
    ".globl _js_load_module",
    ".weak_definition _js_load_module",
    ".p2align 2",
    "_js_load_module:",
    "    mov x0, xzr",
    "    ret",
    // js_call_function(i64, i64, i64, i64, i64) -> f64 ;  return TAG_UNDEFINED
    ".globl _js_call_function",
    ".weak_definition _js_call_function",
    ".p2align 2",
    "_js_call_function:",
    "    mov x0, #1",
    "    movk x0, #0x7FFC, lsl #48",
    "    fmov d0, x0",
    "    ret",
    // js_get_export(i64, i64, i64) -> f64
    ".globl _js_get_export",
    ".weak_definition _js_get_export",
    ".p2align 2",
    "_js_get_export:",
    "    mov x0, #1",
    "    movk x0, #0x7FFC, lsl #48",
    "    fmov d0, x0",
    "    ret",
    // js_set_property(f64, i64, i64, f64) -> void
    ".globl _js_set_property",
    ".weak_definition _js_set_property",
    ".p2align 2",
    "_js_set_property:",
    "    ret",
    // js_runtime_init() -> void
    ".globl _js_runtime_init",
    ".weak_definition _js_runtime_init",
    ".p2align 2",
    "_js_runtime_init:",
    "    ret",
    // js_new_from_handle(f64, i64, i64) -> f64
    ".globl _js_new_from_handle",
    ".weak_definition _js_new_from_handle",
    ".p2align 2",
    "_js_new_from_handle:",
    "    mov x0, #1",
    "    movk x0, #0x7FFC, lsl #48",
    "    fmov d0, x0",
    "    ret",
    // js_new_instance(i64, i64, i64, i64, i64) -> f64
    ".globl _js_new_instance",
    ".weak_definition _js_new_instance",
    ".p2align 2",
    "_js_new_instance:",
    "    mov x0, #1",
    "    movk x0, #0x7FFC, lsl #48",
    "    fmov d0, x0",
    "    ret",
    // js_create_callback(i64, i64, i64) -> f64
    ".globl _js_create_callback",
    ".weak_definition _js_create_callback",
    ".p2align 2",
    "_js_create_callback:",
    "    mov x0, #1",
    "    movk x0, #0x7FFC, lsl #48",
    "    fmov d0, x0",
    "    ret",
    // js_await_js_promise(f64) -> f64
    ".globl _js_await_js_promise",
    ".weak_definition _js_await_js_promise",
    ".p2align 2",
    "_js_await_js_promise:",
    "    mov x0, #1",
    "    movk x0, #0x7FFC, lsl #48",
    "    fmov d0, x0",
    "    ret",
);

// macOS x86_64: same idea, x86_64 SysV ABI returns f64 in xmm0, i64 in rax.
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
core::arch::global_asm!(
    ".globl _js_load_module",
    ".weak_definition _js_load_module",
    "_js_load_module:",
    "    xor eax, eax",
    "    ret",
    ".globl _js_call_function",
    ".weak_definition _js_call_function",
    "_js_call_function:",
    "    movabs rax, 0x7FFC000000000001",
    "    movq xmm0, rax",
    "    ret",
    ".globl _js_get_export",
    ".weak_definition _js_get_export",
    "_js_get_export:",
    "    movabs rax, 0x7FFC000000000001",
    "    movq xmm0, rax",
    "    ret",
    ".globl _js_set_property",
    ".weak_definition _js_set_property",
    "_js_set_property:",
    "    ret",
    ".globl _js_runtime_init",
    ".weak_definition _js_runtime_init",
    "_js_runtime_init:",
    "    ret",
    ".globl _js_new_from_handle",
    ".weak_definition _js_new_from_handle",
    "_js_new_from_handle:",
    "    movabs rax, 0x7FFC000000000001",
    "    movq xmm0, rax",
    "    ret",
    ".globl _js_new_instance",
    ".weak_definition _js_new_instance",
    "_js_new_instance:",
    "    movabs rax, 0x7FFC000000000001",
    "    movq xmm0, rax",
    "    ret",
    ".globl _js_create_callback",
    ".weak_definition _js_create_callback",
    "_js_create_callback:",
    "    movabs rax, 0x7FFC000000000001",
    "    movq xmm0, rax",
    "    ret",
    ".globl _js_await_js_promise",
    ".weak_definition _js_await_js_promise",
    "_js_await_js_promise:",
    "    movabs rax, 0x7FFC000000000001",
    "    movq xmm0, rax",
    "    ret",
);

// Non-macOS platforms: plain Rust stubs. Signatures match codegen declarations
// in `crates/perry-codegen/src/runtime_decls.rs` (caller register
// agreement). Returns TAG_UNDEFINED for f64 returns, 0 for i64 returns.
#[cfg(not(target_os = "macos"))]
#[no_mangle]
pub extern "C" fn js_load_module(_path_ptr: i64, _path_len: i64) -> i64 {
    0
}

#[cfg(not(target_os = "macos"))]
#[no_mangle]
pub extern "C" fn js_call_function(
    _module_handle: i64,
    _name_ptr: i64,
    _name_len: i64,
    _args_ptr: i64,
    _args_len: i64,
) -> f64 {
    f64::from_bits(_UNDEF_BITS)
}

#[cfg(not(target_os = "macos"))]
#[no_mangle]
pub extern "C" fn js_get_export(_module: i64, _name_ptr: i64, _name_len: i64) -> f64 {
    f64::from_bits(_UNDEF_BITS)
}

#[cfg(not(target_os = "macos"))]
#[no_mangle]
pub extern "C" fn js_set_property(_obj: f64, _key_ptr: i64, _key_len: i64, _value: f64) {}

#[cfg(not(target_os = "macos"))]
#[no_mangle]
pub extern "C" fn js_runtime_init() {}

#[cfg(not(target_os = "macos"))]
#[no_mangle]
pub extern "C" fn js_new_from_handle(_constructor: f64, _args_ptr: i64, _args_len: i64) -> f64 {
    f64::from_bits(_UNDEF_BITS)
}

#[cfg(not(target_os = "macos"))]
#[no_mangle]
pub extern "C" fn js_new_instance(
    _class_ptr: i64,
    _name_ptr: i64,
    _name_len: i64,
    _args_ptr: i64,
    _args_len: i64,
) -> f64 {
    f64::from_bits(_UNDEF_BITS)
}

#[cfg(not(target_os = "macos"))]
#[no_mangle]
pub extern "C" fn js_create_callback(_func_ptr: i64, _closure_env: i64, _param_count: i64) -> f64 {
    f64::from_bits(_UNDEF_BITS)
}

#[cfg(not(target_os = "macos"))]
#[no_mangle]
pub extern "C" fn js_await_js_promise(_promise: f64) -> f64 {
    f64::from_bits(_UNDEF_BITS)
}

// =============================================================================
// AOT stubs for unconditionally-declared extern functions
// =============================================================================

#[no_mangle]
pub extern "C" fn js_ratelimit_create() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_ends_with() -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn js_lodash_escape() -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn js_lodash_includes() -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn js_lodash_lower_first() -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn js_lodash_replace() -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn js_lodash_split() -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn js_lodash_start_case() -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn js_lodash_starts_with() -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn js_lodash_unescape() -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn js_lodash_upper_first() -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn js_axios_create() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_axios_request() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_argon2_hash_options() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_sharp_negate() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_sharp_quality() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_sharp_to_format() -> i64 {
    0
}
// js_sqlite_transaction / _commit / _rollback stubs removed — the real
// implementations live in perry-stdlib/src/sqlite.rs and would collide at
// link time when both crates are present (e.g. `cargo test --workspace`).
