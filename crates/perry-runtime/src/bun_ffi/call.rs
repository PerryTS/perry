//! Typed C-ABI calls: argument marshalling + register/stack call shims.
//!
//! ## Why not libffi
//!
//! Every supported `FFIType` is a scalar (integer, float, or pointer), so the
//! full generality of libffi (struct classification, closures) buys nothing
//! here, while costing a new native library on every final link (perry's
//! driver links user binaries with `cc`; libffi would have to be added to
//! every platform link line and vendored for cross builds — including the
//! HarmonyOS/cross targets, where a prebuilt `libffi` is not a given).
//! Instead we exploit how the two supported ABIs assign scalar arguments:
//!
//! - **SysV x86-64**: integer-class args take rdi, rsi, rdx, rcx, r8, r9 in
//!   order (then the stack, left to right); float-class args take xmm0–xmm7
//!   in order. The two register files are assigned INDEPENDENTLY.
//! - **AAPCS64 (incl. Apple arm64)**: integer-class args take x0–x7; float
//!   args take v0–v7. Also independent.
//!
//! Arguments that exhaust their register class continue on the stack in
//! original source order. That last clause matters: OpenTUI's complete symbol
//! table contains 14-argument functions and FFF contains 13-argument
//! functions, so a register-only implementation cannot even finish `dlopen`.
//! The assembly helpers below construct the exact register and stack image
//! directly. They never transmute a native symbol to a mismatched Rust
//! function type and never pass surplus arguments.
//!
//! ### Residual assumptions (documented, not eliminated)
//!
//! - **Wide-register int passing**: narrower integer-class args
//!   (bool/i8/i16/i32/char/ptr/cstring) are passed through `usize` (64-bit)
//!   slots, zero/sign-extended to 64 bits during marshalling. On both ABIs a
//!   narrow integer arg occupies a full integer register and the callee
//!   reads the low bits, so this matches the C prototype at the ABI level —
//!   but it is a `fn(usize)` ⇄ `fn(int32_t)` type pun that only a native-ABI
//!   FFI (or libffi) can make. This is the fundamental FFI assumption; the
//!   full-blessing alternative is libffi, deferred (see above).
//! - **f32 args** are passed in the low 32 bits of a vector register, so an
//!   `f32` value `v` is smuggled through an `f64` slot as
//!   `f64::from_bits(v.to_bits() as u64)`; the callee's `s`/`xmm` read sees
//!   the correct single-precision pattern. (`v as f64` would be wrong.)
//! - **Narrow returns** (bool/i8/u8/i16/u16/i32/u32): only the low bits of
//!   the return register are specified, so we truncate to the declared width
//!   before boxing.
//! - **Variadics** are unsupported (Apple arm64 passes variadic args on the
//!   stack) — a limitation shared with Bun's documented FFI surface.
//!
//! `dlopen` enforces the public 16-scalar-argument cap and rejects unsupported
//! targets up front.

use super::types::*;
use crate::value::JSValue;

pub(crate) const MAX_FLOAT_ARGS: usize = 8;
/// Total JS-visible parameter cap (drives the per-arity closure thunks).
pub(crate) const MAX_ARGS: usize = 16;

#[cfg(target_arch = "x86_64")]
const ABI_INT_REGS: usize = 6;
#[cfg(target_arch = "aarch64")]
const ABI_INT_REGS: usize = 8;
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
const ABI_INT_REGS: usize = 0;

/// Marshalled register image for one call.
pub(crate) struct ArgImage {
    pub ints: [usize; 8],
    pub floats: [f64; MAX_FLOAT_ARGS],
    /// Arguments that did not fit their ABI register class, in original
    /// declaration order, including the target ABI's padding.
    pub stack: [u8; MAX_ARGS * 8],
    /// Number of populated register and stack slots.
    pub n_int: usize,
    pub n_float: usize,
    pub n_stack_bytes: usize,
    /// NUL-terminated temporaries for `cstring` args passed as JS strings.
    /// Kept alive until after the native call returns.
    pub temps: Vec<Vec<u8>>,
}

impl Default for ArgImage {
    fn default() -> Self {
        Self {
            ints: [0; 8],
            floats: [0.0; MAX_FLOAT_ARGS],
            stack: [0; MAX_ARGS * 8],
            n_int: 0,
            n_float: 0,
            n_stack_bytes: 0,
            temps: Vec::new(),
        }
    }
}

/// True when this build can actually issue FFI calls. Kept as a function so
/// `dlopen` can throw one descriptive error on unsupported targets instead
/// of scattering cfg's.
pub(crate) const fn platform_supported() -> bool {
    cfg!(all(
        unix,
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))
}

#[cfg(target_vendor = "apple")]
macro_rules! call_symbol {
    ($name:literal) => {
        concat!("_", $name)
    };
}
#[cfg(not(target_vendor = "apple"))]
macro_rules! call_symbol {
    ($name:literal) => {
        $name
    };
}

// SysV x86-64 helper ABI on entry:
//   rdi=target, rsi=integer image, rdx=float image, rcx=stack image,
//   r8=stack count. The helper builds the target call frame, then loads all
// target argument registers last so its own bookkeeping cannot clobber them.
#[cfg(all(unix, target_arch = "x86_64"))]
core::arch::global_asm!(
    ".text",
    ".p2align 4, 0x90",
    concat!(".globl ", call_symbol!("perry_ffi_call_scalar_int")),
    concat!(".globl ", call_symbol!("perry_ffi_call_scalar_f64")),
    concat!(".globl ", call_symbol!("perry_ffi_call_scalar_f32")),
    concat!(call_symbol!("perry_ffi_call_scalar_int"), ":"),
    concat!(call_symbol!("perry_ffi_call_scalar_f64"), ":"),
    concat!(call_symbol!("perry_ffi_call_scalar_f32"), ":"),
    "    push rbp",
    "    push r12",
    "    push r13",
    "    push r14",
    "    push r15",
    "    mov r12, rdi",
    "    mov r13, rsi",
    "    mov r14, rdx",
    "    mov r15, rcx",
    "    mov rbp, r8",
    // Round stack byte length up to 16. RSP is 16-aligned after five pushes.
    "    lea r11, [r8 + 15]",
    "    and r11, -16",
    "    sub rsp, r11",
    "    xor r10d, r10d",
    "2:",
    "    cmp r10, rbp",
    "    jae 3f",
    "    mov r11, qword ptr [r15 + r10]",
    "    mov qword ptr [rsp + r10], r11",
    "    add r10, 8",
    "    jmp 2b",
    "3:",
    "    mov rdi, qword ptr [r13 + 0]",
    "    mov rsi, qword ptr [r13 + 8]",
    "    mov rdx, qword ptr [r13 + 16]",
    "    mov rcx, qword ptr [r13 + 24]",
    "    mov r8,  qword ptr [r13 + 32]",
    "    mov r9,  qword ptr [r13 + 40]",
    "    movq xmm0, qword ptr [r14 + 0]",
    "    movq xmm1, qword ptr [r14 + 8]",
    "    movq xmm2, qword ptr [r14 + 16]",
    "    movq xmm3, qword ptr [r14 + 24]",
    "    movq xmm4, qword ptr [r14 + 32]",
    "    movq xmm5, qword ptr [r14 + 40]",
    "    movq xmm6, qword ptr [r14 + 48]",
    "    movq xmm7, qword ptr [r14 + 56]",
    "    call r12",
    "    lea r11, [rbp + 15]",
    "    and r11, -16",
    "    add rsp, r11",
    "    pop r15",
    "    pop r14",
    "    pop r13",
    "    pop r12",
    "    pop rbp",
    "    ret",
);

// AAPCS64 helper ABI on entry: x0=target, x1=integer image, x2=float
// image, x3=stack image, x4=stack count.
#[cfg(all(unix, target_arch = "aarch64"))]
core::arch::global_asm!(
    ".text",
    ".p2align 2",
    concat!(".globl ", call_symbol!("perry_ffi_call_scalar_int")),
    concat!(".globl ", call_symbol!("perry_ffi_call_scalar_f64")),
    concat!(".globl ", call_symbol!("perry_ffi_call_scalar_f32")),
    concat!(call_symbol!("perry_ffi_call_scalar_int"), ":"),
    concat!(call_symbol!("perry_ffi_call_scalar_f64"), ":"),
    concat!(call_symbol!("perry_ffi_call_scalar_f32"), ":"),
    "    stp x29, x30, [sp, #-48]!",
    "    stp x19, x20, [sp, #16]",
    "    stp x21, x22, [sp, #32]",
    "    mov x29, sp",
    "    mov x19, x0",
    "    mov x20, x1",
    "    mov x21, x2",
    "    mov x22, x3",
    "    add x9, x4, #15",
    "    and x9, x9, #-16",
    "    sub sp, sp, x9",
    "    mov x10, #0",
    "2:",
    "    cmp x10, x4",
    "    b.hs 3f",
    "    ldr x11, [x22, x10]",
    "    str x11, [sp, x10]",
    "    add x10, x10, #8",
    "    b 2b",
    "3:",
    "    ldp x0, x1, [x20, #0]",
    "    ldp x2, x3, [x20, #16]",
    "    ldp x4, x5, [x20, #32]",
    "    ldp x6, x7, [x20, #48]",
    "    ldp d0, d1, [x21, #0]",
    "    ldp d2, d3, [x21, #16]",
    "    ldp d4, d5, [x21, #32]",
    "    ldp d6, d7, [x21, #48]",
    "    blr x19",
    "    mov sp, x29",
    "    ldp x19, x20, [sp, #16]",
    "    ldp x21, x22, [sp, #32]",
    "    ldp x29, x30, [sp], #48",
    "    ret",
);

#[cfg(all(unix, any(target_arch = "x86_64", target_arch = "aarch64")))]
mod raw {
    extern "C" {
        #[link_name = "perry_ffi_call_scalar_int"]
        fn scalar_int(
            target: usize,
            ints: *const usize,
            floats: *const f64,
            stack: *const u8,
            stack_len: usize,
        ) -> u64;
        #[link_name = "perry_ffi_call_scalar_f64"]
        fn scalar_f64(
            target: usize,
            ints: *const usize,
            floats: *const f64,
            stack: *const u8,
            stack_len: usize,
        ) -> f64;
        #[link_name = "perry_ffi_call_scalar_f32"]
        fn scalar_f32(
            target: usize,
            ints: *const usize,
            floats: *const f64,
            stack: *const u8,
            stack_len: usize,
        ) -> f32;
    }

    pub(crate) unsafe fn call_int(f: usize, image: &super::ArgImage) -> u64 {
        scalar_int(
            f,
            image.ints.as_ptr(),
            image.floats.as_ptr(),
            image.stack.as_ptr(),
            image.n_stack_bytes,
        )
    }

    pub(crate) unsafe fn call_f64(f: usize, image: &super::ArgImage) -> f64 {
        scalar_f64(
            f,
            image.ints.as_ptr(),
            image.floats.as_ptr(),
            image.stack.as_ptr(),
            image.n_stack_bytes,
        )
    }

    pub(crate) unsafe fn call_f32(f: usize, image: &super::ArgImage) -> f32 {
        scalar_f32(
            f,
            image.ints.as_ptr(),
            image.floats.as_ptr(),
            image.stack.as_ptr(),
            image.n_stack_bytes,
        )
    }
}

#[cfg(not(all(unix, any(target_arch = "x86_64", target_arch = "aarch64"))))]
mod raw {
    // `dlopen` refuses before any symbol closure can exist on these targets;
    // these stubs keep the module compiling.
    pub(crate) unsafe fn call_int(_f: usize, _image: &super::ArgImage) -> u64 {
        unreachable!("bun:ffi call on unsupported target")
    }
    pub(crate) unsafe fn call_f64(_f: usize, _image: &super::ArgImage) -> f64 {
        unreachable!("bun:ffi call on unsupported target")
    }
    pub(crate) unsafe fn call_f32(_f: usize, _image: &super::ArgImage) -> f32 {
        unreachable!("bun:ffi call on unsupported target")
    }
}

// ── JS value → C scalar coercions ───────────────────────────────────────────

/// BigInt → low 64 bits, two's complement (i.e. C `(uint64_t)` / `(int64_t)`
/// wrapping semantics — the limbs already store two's complement).
unsafe fn bigint_low_u64(v: JSValue) -> u64 {
    let addr = crate::value::js_nanbox_get_bigint(f64::from_bits(v.bits()));
    if addr == 0 {
        return 0;
    }
    (*(addr as usize as *const crate::bigint::BigIntHeader)).limbs[0]
}

/// Numeric coercion for integer-typed args. Numbers use Rust's saturating
/// float→int cast (NaN → 0); BigInts wrap mod 2^64 like C; booleans are
/// 0/1; null/undefined are 0. Objects/strings do NOT go through JS ToNumber
/// here — Bun requires numeric-ish args for integer slots too.
pub(crate) unsafe fn value_to_u64_int(v: f64) -> u64 {
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_int32() {
        return jv.as_int32() as i64 as u64;
    }
    if jv.is_number() {
        return jv.as_number() as i64 as u64;
    }
    if jv.is_bigint() {
        return bigint_low_u64(jv);
    }
    if jv.is_bool() {
        return jv.as_bool() as u64;
    }
    0
}

/// Numeric coercion for float-typed args.
pub(crate) unsafe fn value_to_f64_num(v: f64) -> f64 {
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_int32() {
        return jv.as_int32() as f64;
    }
    if jv.is_number() {
        return jv.as_number();
    }
    if jv.is_bigint() {
        return bigint_low_u64(jv) as i64 as f64;
    }
    if jv.is_bool() {
        return if jv.as_bool() { 1.0 } else { 0.0 };
    }
    if jv.is_null() {
        return 0.0;
    }
    f64::NAN
}

/// Resolve a JS value to `(data_ptr, byte_len)` when it is a
/// buffer-of-bytes object: Buffer, ArrayBuffer, SharedArrayBuffer,
/// DataView (all `BufferHeader`-backed) or a registered TypedArray.
///
/// Views resolve through `buffer::view::resolve_data_ptr` so the pointer
/// always targets the ultimate backing bytes (#6515) — see the module doc
/// for the resulting read-back caveat on views.
pub(crate) unsafe fn value_buffer_span(v: f64) -> Option<(*mut u8, usize)> {
    let jv = JSValue::from_bits(v.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let addr = crate::value::js_nanbox_get_pointer(f64::from_bits(jv.bits())) as usize;
    if addr == 0 {
        return None;
    }
    if crate::buffer::is_registered_buffer(addr)
        || crate::buffer::is_any_array_buffer(addr)
        || crate::buffer::is_data_view(addr)
        || crate::buffer::is_uint8array_buffer(addr)
    {
        let buf = addr as *const crate::buffer::BufferHeader;
        let data = crate::buffer::view::resolve_data_ptr(buf);
        return Some((data as *mut u8, (*buf).length as usize));
    }
    if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
        let ta =
            crate::typedarray::clean_ta_ptr(addr as *const crate::typedarray::TypedArrayHeader);
        let bytes = crate::typedarray::typed_array_bytes(ta)?;
        return Some((bytes.as_ptr() as *mut u8, bytes.len()));
    }
    None
}

fn describe_value_for_error(jv: JSValue) -> &'static str {
    if jv.is_any_string() {
        "a string"
    } else if jv.is_bool() {
        "a boolean"
    } else if jv.is_bigint() {
        "a BigInt"
    } else if jv.is_undefined() {
        "undefined"
    } else if jv.is_null() {
        "null"
    } else {
        "the value"
    }
}

/// Pointer-class coercion (`ptr` args). Mirrors Bun: numbers/bigints pass
/// through as addresses, buffer-ish objects hand over their (non-moving)
/// data pointer, null/undefined/0 become NULL, strings are rejected with
/// Bun's exact hint.
pub(crate) unsafe fn value_to_pointer_arg(v: f64) -> usize {
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_undefined() || jv.is_null() {
        return 0;
    }
    if jv.is_bool() {
        return jv.as_bool() as usize;
    }
    if jv.is_int32() {
        return jv.as_int32() as i64 as usize;
    }
    if jv.is_number() {
        return jv.as_number() as i64 as usize;
    }
    if jv.is_bigint() {
        return bigint_low_u64(jv) as usize;
    }
    if let Some((data, _len)) = value_buffer_span(v) {
        return data as usize;
    }
    if jv.is_any_string() {
        crate::fs::validate::throw_type_error_with_code(
            "To convert a string to a pointer, encode it as a buffer",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    crate::fs::validate::throw_type_error_with_code(
        &format!(
            "Unable to convert {} to a pointer",
            describe_value_for_error(jv)
        ),
        "ERR_INVALID_ARG_TYPE",
    )
}

/// `cstring` argument: like `ptr`, but a JS *string* is accepted by making
/// a NUL-terminated UTF-8 copy that lives until the call returns (perry
/// convenience superset — Bun rejects strings; real callers pass Buffers).
unsafe fn value_to_cstring_arg(v: f64, temps: &mut Vec<Vec<u8>>) -> usize {
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_any_string() {
        let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        if let Some(bytes) = crate::string::js_string_key_bytes(jv, &mut sso) {
            let mut owned = Vec::with_capacity(bytes.len() + 1);
            owned.extend_from_slice(bytes);
            owned.push(0);
            let ptr = owned.as_ptr() as usize;
            temps.push(owned);
            return ptr;
        }
    }
    value_to_pointer_arg(v)
}

#[cfg(all(target_vendor = "apple", target_arch = "aarch64"))]
fn stack_size_align(ty: u8) -> (usize, usize) {
    // Apple's arm64 ABI compacts stack arguments at their natural C size
    // (unlike generic AAPCS64 and SysV's eight-byte scalar slots).
    match ty {
        T_BOOL | T_CHAR | T_I8 | T_U8 => (1, 1),
        T_I16 | T_U16 => (2, 2),
        T_I32 | T_U32 | T_F32 => (4, 4),
        _ => (8, 8),
    }
}

#[cfg(not(all(target_vendor = "apple", target_arch = "aarch64")))]
fn stack_size_align(_ty: u8) -> (usize, usize) {
    (8, 8)
}

fn append_stack_arg(image: &mut ArgImage, ty: u8, bits: u64) {
    let (size, align) = stack_size_align(ty);
    let offset = (image.n_stack_bytes + align - 1) & !(align - 1);
    let end = offset + size;
    debug_assert!(end <= image.stack.len());
    image.stack[offset..end].copy_from_slice(&bits.to_ne_bytes()[..size]);
    image.n_stack_bytes = end;
}

/// Marshal `js_args` against the declared `arg_types` into a register
/// image. `js_args` shorter than `arg_types` is padded with undefined
/// (matching JS call semantics); longer is truncated.
///
/// # Safety
/// `arg_types` must have passed `dlopen` validation (≤ 16 total, no
/// napi/buffer types).
pub(crate) unsafe fn marshal_args(arg_types: &[u8], js_args: &[f64]) -> ArgImage {
    let mut image = ArgImage::default();
    let mut ii = 0usize;
    let mut fi = 0usize;
    for (idx, &ty) in arg_types.iter().enumerate() {
        let v = js_args
            .get(idx)
            .copied()
            .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
        let (bits, float_class) = match ty {
            T_F64 => (value_to_f64_num(v).to_bits(), true),
            T_F32 => {
                let f = value_to_f64_num(v) as f32;
                (f.to_bits() as u64, true)
            }
            T_BOOL => (crate::value::js_is_truthy(v) as u64, false),
            T_PTR | T_FUNCTION => (value_to_pointer_arg(v) as u64, false),
            T_CSTRING => (value_to_cstring_arg(v, &mut image.temps) as u64, false),
            // char + all fixed-width integers (incl. usize→u64, the fast
            // variants): the callee reads only its declared width.
            _ => (value_to_u64_int(v), false),
        };
        if float_class {
            if fi < MAX_FLOAT_ARGS {
                image.floats[fi] = f64::from_bits(bits);
                fi += 1;
            } else {
                append_stack_arg(&mut image, ty, bits);
            }
        } else if ii < ABI_INT_REGS {
            image.ints[ii] = bits as usize;
            ii += 1;
        } else {
            append_stack_arg(&mut image, ty, bits);
        }
    }
    image.n_int = ii;
    image.n_float = fi;
    image
}

// ── C scalar → JS value conversions ─────────────────────────────────────────

const MAX_SAFE: i64 = 9_007_199_254_740_991; // 2^53 - 1

fn bool_value(b: bool) -> f64 {
    f64::from_bits(if b {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

pub(crate) fn bigint_value_i64(v: i64) -> f64 {
    crate::value::js_nanbox_bigint(crate::bigint::js_bigint_from_i64(v) as i64)
}

pub(crate) fn bigint_value_u64(v: u64) -> f64 {
    crate::value::js_nanbox_bigint(crate::bigint::js_bigint_from_u64(v) as i64)
}

/// Read a NUL-terminated UTF-8 C string at `addr` into a JS string.
/// (Invalid UTF-8 is replaced lossily — same visible behavior as Bun's
/// `CString`, which decodes via TextDecoder.)
pub(crate) unsafe fn read_cstring_value(addr: usize) -> f64 {
    if addr == 0 {
        return super::null();
    }
    let mut len = 0usize;
    let base = addr as *const u8;
    while *base.add(len) != 0 {
        len += 1;
    }
    let bytes = std::slice::from_raw_parts(base, len);
    match std::str::from_utf8(bytes) {
        Ok(s) => super::string_value(s),
        Err(_) => super::string_value(&String::from_utf8_lossy(bytes)),
    }
}

/// Issue the native call and convert the result per `ret_type`.
///
/// # Safety
/// `fn_ptr` must be a callable C function whose true prototype is scalar,
/// non-variadic, and within the marshalled image's class limits.
pub(crate) unsafe fn call_and_convert(fn_ptr: usize, ret_type: u8, image: &ArgImage) -> f64 {
    call_and_convert_mode(fn_ptr, ret_type, image, false)
}

/// Node's `node:ffi` API exposes pointer results as `bigint`, unlike Bun's
/// number-or-null representation. Keep the machine call identical and vary
/// only the final boxing step.
pub(crate) unsafe fn call_and_convert_node(fn_ptr: usize, ret_type: u8, image: &ArgImage) -> f64 {
    call_and_convert_mode(fn_ptr, ret_type, image, true)
}

unsafe fn call_and_convert_mode(
    fn_ptr: usize,
    ret_type: u8,
    image: &ArgImage,
    pointer_bigint: bool,
) -> f64 {
    let result = match ret_type {
        T_F64 => {
            let r = raw::call_f64(fn_ptr, image);
            super::number_value(r)
        }
        T_F32 => {
            let r = raw::call_f32(fn_ptr, image);
            super::number_value(r as f64)
        }
        T_PTR | T_FUNCTION if pointer_bigint => {
            let r = raw::call_int(fn_ptr, image);
            bigint_value_u64(r)
        }
        _ => {
            let r = raw::call_int(fn_ptr, image);
            convert_int_return(ret_type, r)
        }
    };
    // `image.temps` (cstring temporaries) must outlive the call itself.
    std::hint::black_box(&image.temps);
    result
}

pub(crate) fn convert_int_return(ret_type: u8, r: u64) -> f64 {
    match ret_type {
        T_VOID => super::undefined(),
        T_BOOL => bool_value((r as u8) != 0),
        T_CHAR | T_I8 => super::number_value((r as u8 as i8) as f64),
        T_U8 => super::number_value((r as u8) as f64),
        T_I16 => super::number_value((r as u16 as i16) as f64),
        T_U16 => super::number_value((r as u16) as f64),
        T_I32 => super::number_value((r as u32 as i32) as f64),
        T_U32 => super::number_value((r as u32) as f64),
        // Bun semantics: i64/u64 (and usize, an alias of u64) ALWAYS return
        // BigInt; the `_fast` variants return number while the value is
        // within the safe-integer range.
        T_I64 => bigint_value_i64(r as i64),
        T_U64 => bigint_value_u64(r),
        T_I64_FAST => {
            let v = r as i64;
            if (-MAX_SAFE..=MAX_SAFE).contains(&v) {
                super::number_value(v as f64)
            } else {
                bigint_value_i64(v)
            }
        }
        T_U64_FAST => {
            if r <= MAX_SAFE as u64 {
                super::number_value(r as f64)
            } else {
                bigint_value_u64(r)
            }
        }
        T_PTR | T_FUNCTION => {
            if r == 0 {
                super::null()
            } else {
                // Bun represents pointers as plain JS numbers. Real user-space
                // addresses on the supported targets fit in 52 bits, so the
                // f64 conversion is exact.
                super::number_value(r as f64)
            }
        }
        T_CSTRING => unsafe { read_cstring_value(r as usize) },
        _ => super::undefined(),
    }
}

// ── tests ───────────────────────────────────────────────────────────────────

#[cfg(all(test, unix, any(target_arch = "x86_64", target_arch = "aarch64")))]
mod tests {
    use super::*;

    // Callee prototypes deliberately narrower than the shim table's max —
    // exactly the situation at a real dlopen'd symbol. Each test calls
    // through the EXACT arity (n_int, n_float) the callee declares.

    extern "C" fn sum8_i32(a: i32, b: i32, c: i32, d: i32, e: i32, f: i32, g: i32, h: i32) -> i64 {
        a as i64 + b as i64 + c as i64 + d as i64 + e as i64 + f as i64 + g as i64 + h as i64
    }

    #[allow(clippy::too_many_arguments)]
    extern "C" fn sum14_i32(
        a: i32,
        b: i32,
        c: i32,
        d: i32,
        e: i32,
        f: i32,
        g: i32,
        h: i32,
        i: i32,
        j: i32,
        k: i32,
        l: i32,
        m: i32,
        n: i32,
    ) -> i64 {
        [a, b, c, d, e, f, g, h, i, j, k, l, m, n]
            .into_iter()
            .map(i64::from)
            .sum()
    }

    extern "C" fn dsum8(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, g: f64, h: f64) -> f64 {
        a + b + c + d + e + f + g + h
    }

    extern "C" fn mixed(a: i32, b: f64, c: i32, d: f64, e: i64, f: f32) -> f64 {
        a as f64 + b * 2.0 + c as f64 * 3.0 + d * 4.0 + e as f64 * 5.0 + f as f64 * 6.0
    }

    extern "C" fn f32_half(v: f32) -> f32 {
        v * 0.5
    }

    #[allow(clippy::too_many_arguments)]
    extern "C" fn mixed_stack_order(
        _i1: i32,
        _i2: i32,
        _i3: i32,
        _i4: i32,
        _i5: i32,
        _i6: i32,
        _f1: f64,
        _f2: f64,
        _f3: f64,
        _f4: f64,
        _f5: f64,
        _f6: f64,
        _f7: f64,
        _f8: f64,
        f9: f64,
        i7: i32,
    ) -> f64 {
        f9 * 100.0 + i7 as f64
    }

    #[allow(clippy::too_many_arguments)]
    extern "C" fn opentui_box_shape(
        a1: u32,
        a2: i32,
        a3: i32,
        a4: u32,
        a5: u32,
        a6: *const u8,
        a7: u32,
        a8: *const u8,
        a9: *const u8,
        a10: *const u8,
        a11: *const u8,
        a12: u32,
        a13: *const u8,
        a14: u32,
    ) -> u64 {
        a1 as u64
            + a2 as u64
            + a3 as u64
            + a4 as u64
            + a5 as u64
            + a6 as usize as u64
            + a7 as u64
            + a8 as usize as u64
            + a9 as usize as u64
            + a10 as usize as u64
            + a11 as usize as u64
            + a12 as u64
            + a13 as usize as u64
            + a14 as u64
    }

    extern "C" fn u64_id(v: u64) -> u64 {
        v
    }

    extern "C" fn bool_not(v: bool) -> bool {
        !v
    }

    extern "C" fn i8_neg(v: i8) -> i8 {
        -v
    }

    fn image_from(ints: &[usize], floats: &[f64]) -> ArgImage {
        let mut image = ArgImage::default();
        let int_regs = ints.len().min(ABI_INT_REGS);
        image.ints[..int_regs].copy_from_slice(&ints[..int_regs]);
        image.n_int = int_regs;
        for &value in &ints[int_regs..] {
            append_stack_arg(&mut image, T_I32, value as u64);
        }
        let float_regs = floats.len().min(MAX_FLOAT_ARGS);
        image.floats[..float_regs].copy_from_slice(&floats[..float_regs]);
        image.n_float = float_regs;
        for &value in &floats[float_regs..] {
            append_stack_arg(&mut image, T_F64, value.to_bits());
        }
        image
    }

    #[test]
    fn register_image_reaches_eight_int_args() {
        let image = image_from(&[1, 2, 3, 4, 5, 6, 7, 8], &[]);
        let r = unsafe { raw::call_int(sum8_i32 as *const () as usize, &image) };
        assert_eq!(r as i64, 36);
    }

    #[test]
    fn stack_image_reaches_fourteen_int_args() {
        let image = image_from(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14], &[]);
        assert!(image.n_stack_bytes >= 24);
        let r = unsafe { raw::call_int(sum14_i32 as *const () as usize, &image) };
        assert_eq!(r as i64, 105);
    }

    #[test]
    fn register_image_reaches_eight_float_args() {
        let image = image_from(&[], &[0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5]);
        let r = unsafe { raw::call_f64(dsum8 as *const () as usize, &image) };
        assert_eq!(r, 32.0);
    }

    #[test]
    fn mixed_int_float_assignment_matches_the_abi() {
        // callee: (i32 a, f64 b, i32 c, f64 d, i64 e, f32 f)
        //   ints  → [a, c, e], floats → [b, d, f32-image(f)]
        let f_img = f64::from_bits((1.5f32).to_bits() as u64);
        let image = image_from(&[10, 20, 30], &[2.0, 4.0, f_img]);
        let r = unsafe { raw::call_f64(mixed as *const () as usize, &image) };
        assert_eq!(r, 10.0 + 4.0 + 60.0 + 16.0 + 150.0 + 9.0);
    }

    #[test]
    fn mixed_overflow_arguments_keep_source_stack_order() {
        let types = [
            T_I32, T_I32, T_I32, T_I32, T_I32, T_I32, T_F64, T_F64, T_F64, T_F64, T_F64, T_F64,
            T_F64, T_F64, T_F64, T_I32,
        ];
        let values: Vec<f64> = (1..=16)
            .map(|value| super::super::number_value(value as f64))
            .collect();
        let image = unsafe { marshal_args(&types, &values) };
        #[cfg(target_arch = "x86_64")]
        assert_eq!(image.n_stack_bytes, 16);
        #[cfg(target_arch = "aarch64")]
        assert_eq!(image.n_stack_bytes, 8);
        let result = unsafe { raw::call_f64(mixed_stack_order as *const () as usize, &image) };
        assert_eq!(result, 1516.0);
    }

    #[test]
    fn opentui_fourteen_argument_pointer_alignment_matches_host_abi() {
        let types = [
            T_U32, T_I32, T_I32, T_U32, T_U32, T_PTR, T_U32, T_PTR, T_PTR, T_PTR, T_PTR, T_U32,
            T_PTR, T_U32,
        ];
        let values: Vec<f64> = (1..=14)
            .map(|value| super::super::number_value(value as f64))
            .collect();
        let image = unsafe { marshal_args(&types, &values) };
        let result = unsafe { raw::call_int(opentui_box_shape as *const () as usize, &image) };
        assert_eq!(result, 105);
    }

    #[test]
    fn f32_return_and_f32_bit_image_arg() {
        let image = image_from(&[], &[f64::from_bits((21.0f32).to_bits() as u64)]);
        let r = unsafe { raw::call_f32(f32_half as *const () as usize, &image) };
        assert_eq!(r, 10.5f32);
    }

    #[test]
    fn u64_roundtrip_keeps_all_bits() {
        let image = image_from(&[u64::MAX as usize], &[]);
        let r = unsafe { raw::call_int(u64_id as *const () as usize, &image) };
        assert_eq!(r, u64::MAX);
    }

    #[test]
    fn narrow_returns_truncate_to_declared_width() {
        let image = image_from(&[1], &[]);
        let r = unsafe { raw::call_int(bool_not as *const () as usize, &image) };
        // Only the low byte is specified; the converter masks it.
        assert!(!((r as u8) != 0));

        let image = image_from(&[5], &[]);
        let r = unsafe { raw::call_int(i8_neg as *const () as usize, &image) };
        assert_eq!(r as u8 as i8, -5);
    }

    // Exact-arity dispatch must select the right shim for a callee that uses
    // FEWER than the max args — the case the previous over-call trampoline
    // got away with only by ABI luck.
    extern "C" fn add3(a: i32, b: i32, c: i32) -> i64 {
        a as i64 + b as i64 + c as i64
    }
    extern "C" fn noargs() -> i32 {
        1234
    }

    #[test]
    fn exact_arity_three_ints() {
        let image = image_from(&[100, 20, 3], &[]);
        let r = unsafe { raw::call_int(add3 as *const () as usize, &image) };
        assert_eq!(r, 123);
    }

    #[test]
    fn exact_arity_zero_args() {
        let image = image_from(&[], &[]);
        let r = unsafe { raw::call_int(noargs as *const () as usize, &image) };
        assert_eq!(r as u32, 1234);
    }

    #[test]
    fn marshal_records_class_counts() {
        // (i32, f64, i32, f32, ptr) → 3 int-class, 2 float-class
        let image = unsafe {
            marshal_args(
                &[T_I32, T_F64, T_I32, T_F32, T_PTR],
                &[
                    super::super::number_value(1.0),
                    super::super::number_value(2.0),
                    super::super::number_value(3.0),
                    super::super::number_value(4.0),
                    super::super::number_value(0.0),
                ],
            )
        };
        assert_eq!(image.n_int, 3);
        assert_eq!(image.n_float, 2);
    }

    #[test]
    fn int_return_conversion_widths() {
        assert_eq!(
            convert_int_return(T_I8, 0xFFu64),
            super::super::number_value(-1.0)
        );
        assert_eq!(
            convert_int_return(T_U8, 0x1FFu64),
            super::super::number_value(255.0)
        );
        assert_eq!(
            convert_int_return(T_I32, 0xFFFF_FFFFu64),
            super::super::number_value(-1.0)
        );
        assert_eq!(
            convert_int_return(T_U32, 0xFFFF_FFFFu64),
            super::super::number_value(4294967295.0)
        );
        // i64_fast within safe range → number
        assert_eq!(
            convert_int_return(T_I64_FAST, 42u64),
            super::super::number_value(42.0)
        );
        // ptr NULL → null
        assert_eq!(
            convert_int_return(T_PTR, 0).to_bits(),
            crate::value::TAG_NULL
        );
    }

    #[test]
    fn marshal_pads_missing_args_with_zero() {
        let image = unsafe { marshal_args(&[T_I32, T_I32], &[super::super::number_value(7.0)]) };
        assert_eq!(image.ints[0], 7);
        assert_eq!(image.ints[1], 0);
    }

    #[test]
    fn marshal_saturating_and_bool_coercions() {
        unsafe {
            let image = marshal_args(
                &[T_I32, T_BOOL, T_F32],
                &[
                    super::super::number_value(-3.9),
                    f64::from_bits(crate::value::TAG_TRUE),
                    super::super::number_value(1.5),
                ],
            );
            assert_eq!(image.ints[0] as u64 as i64, -3);
            assert_eq!(image.ints[1], 1);
            assert_eq!(image.floats[0].to_bits(), (1.5f32).to_bits() as u64);
        }
    }
}
