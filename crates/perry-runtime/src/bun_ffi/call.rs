//! Typed C-ABI calls: argument marshalling + exact-arity register-image
//! call shims.
//!
//! ## Why not libffi
//!
//! Every stage-1 `FFIType` is a scalar (integer, float, or pointer), so the
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
//! So for a callee prototype made of scalars, packing the marshalled values
//! densely per class and calling through a signature with exactly that many
//! integer-class then float-class parameters reproduces the callee's own
//! register/stack image — integer-class args land in the same integer
//! registers/stack slots and float-class args in the same vector registers,
//! regardless of the callee's original interleaving.
//!
//! ### Exact arity (no over-calling)
//!
//! A previous revision transmuted every symbol to one fixed 16-parameter
//! `fn(usize×8, f64×8)` and relied on the callee ignoring the extra
//! registers/stack. That is an ABI-level truth but NOT blessed by Rust's
//! abstract machine (calling a function pointer whose arity exceeds the real
//! definition's is UB, and the surplus x86-64 stack slots are written into
//! the callee's frame). This revision instead dispatches on the marshalled
//! `(n_int, n_float)` and transmutes to a signature with EXACTLY `n_int`
//! `usize` params followed by `n_float` `f64` params (9 × 9 monomorphic
//! shims per return class, macro-generated below). The callee is therefore
//! never over-called.
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
//! `dlopen` enforces the ≤ 8-int / ≤ 8-float limit and rejects unsupported
//! targets up front, so an out-of-range `(n_int, n_float)` can never reach a
//! shim (the `_` fallbacks below are unreachable in practice).

use super::types::*;
use crate::value::JSValue;

pub(crate) const MAX_INT_ARGS: usize = 8;
pub(crate) const MAX_FLOAT_ARGS: usize = 8;
/// Total JS-visible parameter cap (drives the per-arity closure thunks).
pub(crate) const MAX_ARGS: usize = 16;

/// Marshalled register image for one call.
#[derive(Default)]
pub(crate) struct ArgImage {
    pub ints: [usize; MAX_INT_ARGS],
    pub floats: [f64; MAX_FLOAT_ARGS],
    /// Number of populated integer-class / float-class slots — the exact
    /// arity the call shim transmutes to.
    pub n_int: usize,
    pub n_float: usize,
    /// NUL-terminated temporaries for `cstring` args passed as JS strings.
    /// Kept alive until after the native call returns.
    pub temps: Vec<Vec<u8>>,
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

#[cfg(all(unix, any(target_arch = "x86_64", target_arch = "aarch64")))]
mod raw {
    //! Exact-arity call shims. `call_{int,f64,f32}` dispatch on
    //! `(n_int, n_float)` and transmute the symbol to a signature with
    //! precisely `n_int` `usize` params then `n_float` `f64` params — never
    //! more than the callee actually declares.

    // Map an index token to the slot's Rust ABI type (value ignored).
    macro_rules! ty_usize {
        ($t:tt) => {
            usize
        };
    }
    macro_rules! ty_f64 {
        ($t:tt) => {
            f64
        };
    }

    /// Transmute `$f` to `extern "C" fn(usize×|ints| , f64×|floats|) -> $ret`
    /// and call it with exactly those slots. Trailing commas make the empty
    /// list (`fn() -> $ret`) valid.
    macro_rules! call_exact {
        ($f:expr, $i:ident, $d:ident, $ret:ty, [$($ix:tt)*], [$($fx:tt)*]) => {{
            let g: unsafe extern "C" fn($(ty_usize!($ix),)* $(ty_f64!($fx),)*) -> $ret =
                ::core::mem::transmute($f);
            g($($i[$ix],)* $($d[$fx],)*)
        }};
    }

    /// Inner dispatch over the float-arg count for a fixed int-index list.
    macro_rules! inner_floats {
        ($f:expr, $i:ident, $d:ident, $ret:ty, [$($ix:tt)*], $nf:expr) => {
            match $nf {
                0 => call_exact!($f, $i, $d, $ret, [$($ix)*], []),
                1 => call_exact!($f, $i, $d, $ret, [$($ix)*], [0]),
                2 => call_exact!($f, $i, $d, $ret, [$($ix)*], [0 1]),
                3 => call_exact!($f, $i, $d, $ret, [$($ix)*], [0 1 2]),
                4 => call_exact!($f, $i, $d, $ret, [$($ix)*], [0 1 2 3]),
                5 => call_exact!($f, $i, $d, $ret, [$($ix)*], [0 1 2 3 4]),
                6 => call_exact!($f, $i, $d, $ret, [$($ix)*], [0 1 2 3 4 5]),
                7 => call_exact!($f, $i, $d, $ret, [$($ix)*], [0 1 2 3 4 5 6]),
                _ => call_exact!($f, $i, $d, $ret, [$($ix)*], [0 1 2 3 4 5 6 7]),
            }
        };
    }

    /// Outer dispatch over the int-arg count, then the float count. Expands to
    /// 81 exact-arity transmute+call sites for the given return type.
    macro_rules! dispatch_exact {
        ($f:expr, $i:ident, $d:ident, $ret:ty, $ni:expr, $nf:expr) => {
            match $ni {
                0 => inner_floats!($f, $i, $d, $ret, [], $nf),
                1 => inner_floats!($f, $i, $d, $ret, [0], $nf),
                2 => inner_floats!($f, $i, $d, $ret, [0 1], $nf),
                3 => inner_floats!($f, $i, $d, $ret, [0 1 2], $nf),
                4 => inner_floats!($f, $i, $d, $ret, [0 1 2 3], $nf),
                5 => inner_floats!($f, $i, $d, $ret, [0 1 2 3 4], $nf),
                6 => inner_floats!($f, $i, $d, $ret, [0 1 2 3 4 5], $nf),
                7 => inner_floats!($f, $i, $d, $ret, [0 1 2 3 4 5 6], $nf),
                _ => inner_floats!($f, $i, $d, $ret, [0 1 2 3 4 5 6 7], $nf),
            }
        };
    }

    #[inline(never)]
    pub(crate) unsafe fn call_int(
        f: usize,
        ni: usize,
        i: &[usize; 8],
        nf: usize,
        d: &[f64; 8],
    ) -> u64 {
        dispatch_exact!(f, i, d, u64, ni, nf)
    }

    #[inline(never)]
    pub(crate) unsafe fn call_f64(
        f: usize,
        ni: usize,
        i: &[usize; 8],
        nf: usize,
        d: &[f64; 8],
    ) -> f64 {
        dispatch_exact!(f, i, d, f64, ni, nf)
    }

    #[inline(never)]
    pub(crate) unsafe fn call_f32(
        f: usize,
        ni: usize,
        i: &[usize; 8],
        nf: usize,
        d: &[f64; 8],
    ) -> f32 {
        dispatch_exact!(f, i, d, f32, ni, nf)
    }
}

#[cfg(not(all(unix, any(target_arch = "x86_64", target_arch = "aarch64"))))]
mod raw {
    // `dlopen` refuses before any symbol closure can exist on these targets;
    // these stubs keep the module compiling.
    pub(crate) unsafe fn call_int(
        _f: usize,
        _ni: usize,
        _i: &[usize; 8],
        _nf: usize,
        _d: &[f64; 8],
    ) -> u64 {
        unreachable!("bun:ffi call on unsupported target")
    }
    pub(crate) unsafe fn call_f64(
        _f: usize,
        _ni: usize,
        _i: &[usize; 8],
        _nf: usize,
        _d: &[f64; 8],
    ) -> f64 {
        unreachable!("bun:ffi call on unsupported target")
    }
    pub(crate) unsafe fn call_f32(
        _f: usize,
        _ni: usize,
        _i: &[usize; 8],
        _nf: usize,
        _d: &[f64; 8],
    ) -> f32 {
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
unsafe fn value_to_u64_int(v: f64) -> u64 {
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
unsafe fn value_to_f64_num(v: f64) -> f64 {
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
unsafe fn value_to_pointer_arg(v: f64) -> usize {
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

/// Marshal `js_args` against the declared `arg_types` into a register
/// image. `js_args` shorter than `arg_types` is padded with undefined
/// (matching JS call semantics); longer is truncated.
///
/// # Safety
/// `arg_types` must have passed `dlopen` validation (≤ 8 per class, no
/// function/napi/buffer types).
pub(crate) unsafe fn marshal_args(arg_types: &[u8], js_args: &[f64]) -> ArgImage {
    let mut image = ArgImage::default();
    let mut ii = 0usize;
    let mut fi = 0usize;
    for (idx, &ty) in arg_types.iter().enumerate() {
        let v = js_args
            .get(idx)
            .copied()
            .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
        match ty {
            T_F64 => {
                image.floats[fi] = value_to_f64_num(v);
                fi += 1;
            }
            T_F32 => {
                let f = value_to_f64_num(v) as f32;
                image.floats[fi] = f64::from_bits(f.to_bits() as u64);
                fi += 1;
            }
            T_BOOL => {
                image.ints[ii] = crate::value::js_is_truthy(v) as usize;
                ii += 1;
            }
            T_PTR => {
                image.ints[ii] = value_to_pointer_arg(v);
                ii += 1;
            }
            T_CSTRING => {
                image.ints[ii] = value_to_cstring_arg(v, &mut image.temps);
                ii += 1;
            }
            // char + all fixed-width integers (incl. usize→u64, the fast
            // variants): the callee reads only its declared width.
            _ => {
                image.ints[ii] = value_to_u64_int(v) as usize;
                ii += 1;
            }
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

fn bigint_value_i64(v: i64) -> f64 {
    crate::value::js_nanbox_bigint(crate::bigint::js_bigint_from_i64(v) as i64)
}

fn bigint_value_u64(v: u64) -> f64 {
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
    let (ni, nf) = (image.n_int, image.n_float);
    let result = match ret_type {
        T_F64 => {
            let r = raw::call_f64(fn_ptr, ni, &image.ints, nf, &image.floats);
            super::number_value(r)
        }
        T_F32 => {
            let r = raw::call_f32(fn_ptr, ni, &image.ints, nf, &image.floats);
            super::number_value(r as f64)
        }
        _ => {
            let r = raw::call_int(fn_ptr, ni, &image.ints, nf, &image.floats);
            convert_int_return(ret_type, r)
        }
    };
    // `image.temps` (cstring temporaries) must outlive the call itself.
    std::hint::black_box(&image.temps);
    result
}

fn convert_int_return(ret_type: u8, r: u64) -> f64 {
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
        T_PTR => {
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

    extern "C" fn dsum8(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, g: f64, h: f64) -> f64 {
        a + b + c + d + e + f + g + h
    }

    extern "C" fn mixed(a: i32, b: f64, c: i32, d: f64, e: i64, f: f32) -> f64 {
        a as f64 + b * 2.0 + c as f64 * 3.0 + d * 4.0 + e as f64 * 5.0 + f as f64 * 6.0
    }

    extern "C" fn f32_half(v: f32) -> f32 {
        v * 0.5
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
        image.ints[..ints.len()].copy_from_slice(ints);
        image.floats[..floats.len()].copy_from_slice(floats);
        image.n_int = ints.len();
        image.n_float = floats.len();
        image
    }

    #[test]
    fn register_image_reaches_eight_int_args() {
        let image = image_from(&[1, 2, 3, 4, 5, 6, 7, 8], &[]);
        let r = unsafe {
            raw::call_int(
                sum8_i32 as usize,
                image.n_int,
                &image.ints,
                image.n_float,
                &image.floats,
            )
        };
        assert_eq!(r as i64, 36);
    }

    #[test]
    fn register_image_reaches_eight_float_args() {
        let image = image_from(&[], &[0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5]);
        let r = unsafe {
            raw::call_f64(
                dsum8 as usize,
                image.n_int,
                &image.ints,
                image.n_float,
                &image.floats,
            )
        };
        assert_eq!(r, 32.0);
    }

    #[test]
    fn mixed_int_float_assignment_matches_the_abi() {
        // callee: (i32 a, f64 b, i32 c, f64 d, i64 e, f32 f)
        //   ints  → [a, c, e], floats → [b, d, f32-image(f)]
        let f_img = f64::from_bits((1.5f32).to_bits() as u64);
        let image = image_from(&[10, 20, 30], &[2.0, 4.0, f_img]);
        let r = unsafe {
            raw::call_f64(
                mixed as usize,
                image.n_int,
                &image.ints,
                image.n_float,
                &image.floats,
            )
        };
        assert_eq!(r, 10.0 + 4.0 + 60.0 + 16.0 + 150.0 + 9.0);
    }

    #[test]
    fn f32_return_and_f32_bit_image_arg() {
        let image = image_from(&[], &[f64::from_bits((21.0f32).to_bits() as u64)]);
        let r = unsafe {
            raw::call_f32(
                f32_half as usize,
                image.n_int,
                &image.ints,
                image.n_float,
                &image.floats,
            )
        };
        assert_eq!(r, 10.5f32);
    }

    #[test]
    fn u64_roundtrip_keeps_all_bits() {
        let image = image_from(&[u64::MAX as usize], &[]);
        let r = unsafe {
            raw::call_int(
                u64_id as usize,
                image.n_int,
                &image.ints,
                image.n_float,
                &image.floats,
            )
        };
        assert_eq!(r, u64::MAX);
    }

    #[test]
    fn narrow_returns_truncate_to_declared_width() {
        let image = image_from(&[1], &[]);
        let r = unsafe {
            raw::call_int(
                bool_not as usize,
                image.n_int,
                &image.ints,
                image.n_float,
                &image.floats,
            )
        };
        // Only the low byte is specified; the converter masks it.
        assert!(!((r as u8) != 0));

        let image = image_from(&[5], &[]);
        let r = unsafe {
            raw::call_int(
                i8_neg as usize,
                image.n_int,
                &image.ints,
                image.n_float,
                &image.floats,
            )
        };
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
        let r = unsafe {
            raw::call_int(
                add3 as usize,
                image.n_int,
                &image.ints,
                image.n_float,
                &image.floats,
            )
        };
        assert_eq!(r, 123);
    }

    #[test]
    fn exact_arity_zero_args() {
        let image = image_from(&[], &[]);
        let r = unsafe {
            raw::call_int(
                noargs as usize,
                image.n_int,
                &image.ints,
                image.n_float,
                &image.floats,
            )
        };
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
