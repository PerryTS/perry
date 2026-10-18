//! Computed keys share the immutable layout index of named reads and stores.
use super::*;

/// The canonical word and hash of a computed property key, as a site key
/// would carry them: a property atom or a symbol as it is; a short or plain
/// string as its text's property atom when one exists. `None` for anything
/// else (a number or an object whose ToPropertyKey runs user code). A heap
/// string without an atom keeps its borrowed text and computed hash.
#[inline]
pub(super) unsafe fn computed_key(key_bits: u64) -> Option<(u64, u64)> {
    if let Some(h) = key_word_hash(key_bits) {
        if key_bits & TAG_MASK == STRING_TAG {
            let s = (key_bits & POINTER_MASK) as *const crate::StringHeader;
            let text =
                std::slice::from_raw_parts(crate::string::string_data(s), (*s).byte_len as usize);
            if index_like(text) {
                return None;
            }
        }
        return Some((key_bits, h));
    }
    let tag = key_bits & TAG_MASK;
    if tag != STRING_TAG && tag != crate::value::SHORT_STRING_TAG {
        return None;
    }
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let text = crate::string::js_string_key_bytes(crate::JSValue::from_bits(key_bits), &mut sso)?;
    if index_like(text) {
        return None;
    }
    let h = crate::object::key_bytes_hash(text.as_ptr(), text.len());
    match crate::string::atom_lookup(text, h) {
        Some(a) if crate::string::is_property_atom(a) => Some((STRING_TAG | a as u64, h)),
        // A heap string can probe TEXT entries by its bytes. No atom exists
        // for this text, so an atom-only list also proves absence. Keep short
        // strings on their established atom route: intrinsic readers accept
        // heap strings and Symbols, not inline payloads.
        None if tag == STRING_TAG => Some((key_bits, h)),
        _ => None,
    }
}

/// `obj[key]` for a computed `key` (a string or symbol value): the probe
/// with the key's atom, or `TAG_HOLE` for the site's complete keyed read.
///
/// # Safety
/// `dir` as for [`perry_probe_get`]; any value bits.
#[no_mangle]
pub unsafe extern "C" fn perry_probe_get_keyed(
    dir: *const u8,
    obj_bits: u64,
    key_bits: u64,
) -> f64 {
    if obj_bits & TAG_MASK != POINTER_TAG {
        return hole();
    }
    let payload = (obj_bits & POINTER_MASK) as usize;
    if payload < perry_abi::RECEIVER_HANDLE_FLOOR {
        return hole();
    }
    let Some((key, hash)) = computed_key(key_bits) else {
        return hole();
    };
    let v = probe_get::<true>(
        dir,
        (payload - perry_abi::RECEIVER_HANDLE_FLOOR) as i64,
        key,
        hash,
    );
    v
}

/// Computed-key store through the same immutable index as a named store.
/// Key resolution is allocation-free: an atom carries its creation hash;
/// short/plain strings use an existing atom, and Symbols carry their id.
/// A decline leaves every operand untouched for the complete strict-aware Set.
///
/// # Safety
/// As for [`perry_probe_set`], except `key_bits` can be any JS value.
#[no_mangle]
pub unsafe extern "C" fn perry_probe_set_keyed(
    dir: *const u8,
    obj_bits: u64,
    key_bits: u64,
    value: f64,
) -> i32 {
    let Some((key, hash)) = computed_key(key_bits) else {
        return 0;
    };
    super::store::probe_set::<false>(dir, obj_bits, key, hash, value)
}

#[cfg(test)]
#[path = "layout_probe_computed_tests.rs"]
mod tests;
