//! Per-site result cache for `"literal" + smallInt` — the
//! `obj["field_" + j]` key shape of `bench_object_property`.
//!
//! Codegen (`perry-codegen/src/concat_site_cache.rs`) gives every
//! literal-prefix `+ value` site a private zero-initialised
//! `[CONCAT_SITE_SLOTS x i64]` table and probes it inline. Slot `k` is either
//! 0 or the NaN-boxed heap string `prefix + String(k)` — by construction: the
//! prefix is a source literal, so it never varies at a site, and only this
//! helper writes the table. A hit therefore needs no hashing, no byte compare
//! and no governor. That is the whole gain over the process-wide
//! [`super::concat`] memo (512 slots keyed by content hash, verified by a byte
//! compare, throttled by a windowed governor), whose hit path is still a call
//! into an ~850-instruction function.
//!
//! Slots are write-once. The miss arm fills a slot with exactly the value
//! [`js_string_concat_value_box`] returns and registers the slot's address
//! through [`crate::gc::js_gc_register_global_root`] — the funnel
//! module-global string literals already use — so an evacuating collection
//! rewrites the slot instead of leaving it holding a moved string's old
//! address. An SSO result (≤5 ASCII bytes, e.g. `"k" + 4`) is an immediate
//! with content-stable bits and is cached by value, without a registration:
//! it carries no pointer, and leaving it uncached would send every call of
//! such a site through the fill arm forever.
//!
//! Sharing a result between callers is what the memo does too. Strings are
//! immutable; in-place append (`s += x`) is only taken on values codegen
//! proves uniquely owned, which a handle read back from a table never is.
//!
//! Like module-global roots, a table is process-global while `GLOBAL_ROOTS`
//! is per-thread; compiled module code runs on the thread that registers it.

use super::concat::js_string_concat_value_box;
use crate::string::StringHeader;
use crate::value::JSValue;

/// Slots per site. Must match `CONCAT_SITE_SLOTS` in
/// `perry-codegen/src/concat_site_cache.rs`. `"field_" + j` for `j < 20` is
/// the motivating shape; 32 keeps a site's table to 256 bytes of BSS.
pub const CONCAT_SITE_SLOTS: usize = 32;

/// Which slot a right operand selects: a non-negative integer below the slot
/// count and nothing else. Ordered comparisons reject every NaN, and every
/// NaN-boxed non-number is a NaN. `-0` selects slot 0, which is right: JS
/// prints `-0` as `"0"`.
#[inline]
fn concat_site_slot(value: f64) -> Option<usize> {
    if !(value >= 0.0 && value < CONCAT_SITE_SLOTS as f64) {
        return None;
    }
    let k = value as usize;
    if k as f64 != value {
        return None;
    }
    Some(k)
}

/// Fill arm of the per-site concat cache.
///
/// Answers exactly what [`js_string_concat_value_box`]`(prefix, value)` does
/// for every input, so callers may route any value here; codegen only routes
/// values inside the table (an out-of-range value takes the plain fused call
/// directly, paying no extra call level). As a side effect, when `value`
/// selects a slot, the empty slot is filled with the string result — rooted
/// when it is a heap handle, by value when it is an SSO immediate; a slot
/// that is already filled is answered from the table (the emitted probe
/// checked it first, so this is the write-once guarantee, not a second fast
/// path).
///
/// # Safety
/// `table` must point at `CONCAT_SITE_SLOTS` writable `u64` words that live
/// for the rest of the process — codegen emits a private global for each
/// site, and a filled slot's address is handed to the GC as a root.
#[no_mangle]
pub extern "C" fn js_string_concat_site_value(
    table: *mut u64,
    prefix: *const StringHeader,
    value: f64,
) -> f64 {
    let slot = concat_site_slot(value);
    if let Some(k) = slot {
        let cached = unsafe { *table.add(k) };
        if cached != 0 {
            return f64::from_bits(cached);
        }
    }
    let result = js_string_concat_value_box(prefix, value);
    if let Some(k) = slot {
        let bits = result.to_bits();
        let boxed = JSValue::from_bits(bits);
        if boxed.is_any_string() {
            unsafe {
                let cell = table.add(k);
                // GC_STORE_AUDIT(ROOT): a heap handle's cell is registered as
                // a global root right below, which also applies the root
                // heap-word barrier to the value just stored; an SSO
                // immediate holds no pointer and needs neither.
                std::ptr::write(cell, bits);
                if boxed.is_string() {
                    crate::gc::js_gc_register_global_root(cell as i64);
                }
            }
        }
    }
    result
}
