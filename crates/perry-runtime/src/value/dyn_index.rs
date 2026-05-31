//! Tag-aware dynamic index get/set + helpers for ambiguous index access.

use super::*;

/// Tag-aware dynamic index dispatch for `obj[key]` where `obj` has unknown
/// static type. Strings index strings directly; other pointer receivers route
/// through the same polymorphic helper as the generic IndexGet fallback so
/// object numeric keys use `ToPropertyKey` instead of raw field-slot offsets.
#[no_mangle]
pub extern "C" fn js_dyn_index_get(value: f64, index: f64) -> f64 {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);
    if jsval.is_string() || jsval.is_short_string() {
        let s_ptr = js_get_string_pointer_unified(value) as *const crate::StringHeader;
        if s_ptr.is_null() {
            return f64::from_bits(TAG_UNDEFINED);
        }
        let idx_i32 = if index.is_nan() || index.is_infinite() {
            0
        } else {
            index as i32
        };
        let result = crate::string::js_string_char_at(s_ptr, idx_i32);
        if result.is_null() {
            return f64::from_bits(TAG_UNDEFINED);
        }
        return f64::from_bits(JSValue::string_ptr(result).bits());
    }
    let raw_ptr = if jsval.is_pointer() {
        (bits & POINTER_MASK) as usize
    } else if !value.is_nan()
        && bits != 0
        && bits < 0x0001_0000_0000_0000
        && (bits & 0x3) == 0
        && bits >= 0x10000
    {
        bits as usize
    } else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    if raw_ptr < 0x10000 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // Issue #63 / #321 (Effect.runSync→fork SIGBUS): the raw-I64 fallback
    // above accepts arbitrary in-range bits — including denormal f64
    // payloads from non-pointer dataflow (e.g. effect's fiberRefs.ts loop
    // produced `bits ≈ 0x8_0000_0000` which passed every gate but is just
    // a number value, not a real I64 pointer). The unchecked
    // `(*gc_hdr).obj_type` read at the bottom of this fn then crossed
    // the macOS user/kernel boundary at `[raw_ptr - 8]` → SIGBUS.
    //
    // The platform-aware heap range used by `crate::object::is_valid_obj_ptr`
    // covers exactly the address space mimalloc / system malloc actually
    // hand out (macOS host: `[0x200_0000_0000, 0x8000_0000_0000)`; Linux /
    // iOS / Android: `[0x1000, 0x8000_0000_0000)`). Any value with
    // POINTER_TAG that codegen put there is trusted (it asked for a
    // pointer), so this gate only applies to the heuristic fallback.
    if !jsval.is_pointer() && !crate::object::is_valid_obj_ptr(raw_ptr as *const u8) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // Issue #957: if the index itself is a string, route through the
    // by-name object getter. Pre-fix, `obj["foo"]` lowered through
    // `IndexUpdate` re-entered this helper with a NaN-boxed string index
    // and the `index as i32` coercion produced garbage offsets, so
    // `++obj["foo"]` silently returned undefined.
    let idx_bits = index.to_bits();
    let idx_top16 = idx_bits >> 48;
    if idx_top16 == 0x7FFF || idx_top16 == 0x7FF9 {
        let key_ptr = js_get_string_pointer_unified(index) as *const crate::StringHeader;
        if !key_ptr.is_null() {
            return crate::object::js_object_get_field_by_name_f64(
                raw_ptr as *const crate::object::ObjectHeader,
                key_ptr,
            );
        }
        return f64::from_bits(TAG_UNDEFINED);
    }
    crate::object::js_object_get_index_polymorphic(raw_ptr as i64, index)
}

/// Issue #957 — tag-aware dynamic index write counterpart to
/// `js_dyn_index_get`. Used by `Expr::IndexUpdate` codegen to write back
/// the incremented value without duplicating the IndexSet dispatch tree.
///
/// Routes by the receiver's `gc_type` byte: arrays go through
/// `js_array_set_index_or_string` (numeric/string-key spec dispatch);
/// everything else stringifies the index and routes through
/// `js_object_set_field_by_name`. Strings are immutable — no-op (matches
/// strict-mode `s[i] = x` semantics, close enough for the `++result[key]`
/// pattern this is added for).
#[no_mangle]
pub extern "C" fn js_dyn_index_set(obj: f64, index: f64, value: f64) -> f64 {
    let bits = obj.to_bits();
    let jsval = JSValue::from_bits(bits);
    if jsval.is_string() || jsval.is_short_string() {
        return value;
    }
    let raw_ptr = if jsval.is_pointer() {
        (bits & POINTER_MASK) as usize
    } else if !obj.is_nan()
        && bits != 0
        && bits < 0x0001_0000_0000_0000
        && (bits & 0x3) == 0
        && bits >= 0x10000
    {
        bits as usize
    } else {
        return value;
    };
    if raw_ptr < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return value;
    }
    // Mirror the #63/#321 guard on the get side: heuristic-derived
    // pseudo-pointers from non-pointer dataflow must not be dereferenced.
    if !jsval.is_pointer() && !crate::object::is_valid_obj_ptr(raw_ptr as *const u8) {
        return value;
    }
    let is_array = unsafe {
        let gc_header =
            (raw_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY
    };
    if is_array {
        crate::array::js_array_set_index_or_string(
            raw_ptr as *mut crate::array::ArrayHeader,
            index,
            value,
        );
        return value;
    }
    // Non-array object: stringify the index and write via the object setter.
    let bits = index.to_bits();
    let top16 = bits >> 48;
    let key_ptr: *const crate::StringHeader = if top16 == 0x7FFF {
        (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader
    } else if top16 == 0x7FF9 {
        crate::value::js_get_string_pointer_unified(index) as *const crate::StringHeader
    } else {
        // Numeric (or other) index — stringify and intern as a UTF-8 key.
        let idx_i32 = if index.is_nan() || index.is_infinite() {
            0
        } else {
            index as i32
        };
        let s = idx_i32.to_string();
        crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
    };
    if key_ptr.is_null() {
        return value;
    }
    crate::object::js_object_set_field_by_name(
        raw_ptr as *mut crate::object::ObjectHeader,
        key_ptr,
        value,
    );
    value
}

/// Check if a value should trigger a destructuring default.
/// Returns 1 if the value is TAG_UNDEFINED, or a bare IEEE NaN (e.g., from
/// out-of-bounds array read), 0 otherwise. All other NaN-boxed values
/// (strings, pointers, booleans, etc.) return 0 because their NaN payload
/// does not match NaN or TAG_UNDEFINED exactly.
#[no_mangle]
pub extern "C" fn js_is_undefined_or_bare_nan(value: f64) -> i32 {
    let bits = value.to_bits();
    // TAG_UNDEFINED = 0x7FFC_0000_0000_0001
    if bits == 0x7FFC_0000_0000_0001 {
        return 1;
    }
    // Bare IEEE NaN (0.0/0.0) — produced by OOB array reads
    // Canonical NaN is 0x7FF8_0000_0000_0000 on most platforms
    if bits == 0x7FF8_0000_0000_0000 {
        return 1;
    }
    0
}

// --- #1561: force-keep the dynamic-index FFI exports under LTO ---
//
// `js_dyn_index_get` / `js_dyn_index_set` / `js_is_undefined_or_bare_nan`
// are `#[no_mangle] pub extern "C"`, but they have **zero internal Rust
// callers** — they are only ever invoked from generated LLVM IR (codegen
// emits the calls in `perry-codegen/src/expr/index_get.rs` and
// `expr/instance_misc1.rs`). The default `.a` staticlib keeps them via
// staticlib-export semantics, but any build mode that round-trips the
// runtime through whole-program LLVM bitcode — the `PERRY_LLVM_BITCODE_LINK`
// path in `optimized_libs.rs`, cross-compile `-Zbuild-std` builds, or a
// future switch to fat LTO — is free to *internalize* an unreferenced
// `#[no_mangle]` symbol and dead-strip it, leaving the codegen-emitted call
// dangling: `Undefined symbols: _js_dyn_index_get` at final link.
//
// The `#[used]` statics below take the address of each export, creating a
// retained reference edge that LTO and the linker's `-dead_strip` must
// honor (the entries land in `@llvm.used` / a `no_dead_strip` section). This
// guarantees the symbols survive auto-optimize regardless of feature set or
// link mode. Function-pointer types are `Sync`, so no wrapper is needed.
#[used]
static KEEP_JS_DYN_INDEX_GET: extern "C" fn(f64, f64) -> f64 = js_dyn_index_get;
#[used]
static KEEP_JS_DYN_INDEX_SET: extern "C" fn(f64, f64, f64) -> f64 = js_dyn_index_set;
#[used]
static KEEP_JS_IS_UNDEFINED_OR_BARE_NAN: extern "C" fn(f64) -> i32 = js_is_undefined_or_bare_nan;
