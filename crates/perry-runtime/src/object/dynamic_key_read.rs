//! Computed string-key reads, `o[k]` with a runtime string `k` (#10753).
//!
//! A static read site holds its key's ATOM and compares the receiver's ShapeId
//! against its cache; a computed read has neither, so it used to resolve every
//! read by name: validate the key's UTF-8, hash it for the accessor Bloom,
//! classify the receiver, byte-compare the key against the shape's key list —
//! and, for a key the receiver lacks, repeat the whole generic Get on
//! `%Object.prototype%`. That was 808 instructions for a present key and about
//! 8,500 for an absent one.
//!
//! [`js_typed_feedback_object_get_field_by_key_f64`] answers from shapes first,
//! in this order, and only then takes the generic by-value entry:
//!
//! 1. **The receiver's shape, by key word.** The receiver's ShapeId (its `+4`
//!    word) names an ordinary record whose leading POSBOUND key positions ARE
//!    its inline slots; a canonical key list holds each key's ATOM, which is the
//!    very value a pooled key literal evaluates to, and a key taken from the
//!    list itself (`Object.keys`, `for…in`) is the list's own word. So the key
//!    is compared as one word per position, the megamorphic read front's
//!    confirm (`ic_miss::read_confirm`) without a site guess. A match is the
//!    property; a mismatch proves nothing (a non-atom string of the same text)
//!    and falls through.
//! 2. **The megamorphic read stub**, on the key's CONTENT bits: the own slots
//!    the generic lane and this entry prime (spill and overflow slots,
//!    non-atom keys), and the confirmed ABSENT verdicts this entry primes
//!    (below).
//! 3. **The receiver's own key list, once.** The same lookup that proves a key
//!    absent finds a present one: its slot is read and filed in the stub, so
//!    the generic Get never repeats the lookup for an own data key.
//!
//! # Absent keys
//!
//! An absent key is answered by the receiver's ShapeId plus
//! `%Object.prototype%`'s, the facts of a depth-1 ABSENT holder entry
//! (`method_site::read_holder`). Before the generic Get, the shapes are asked
//! whether they prove the key absent
//! (`read_holder::dynamic_absent_terminal`, allocation-free); after it, an
//! `undefined` answer files the verdict in the stub under the receiver's
//! token. Everything the verdict needs is a value — the token, the key's
//! content bits and the terminal ShapeId — so nothing is held across the Get,
//! which can collect.
//!
//! Every answer here is GC-free: no allocation, no collection, no user code.

use super::ObjectHeader;

/// Inline slot `slot` of `obj`.
#[inline(always)]
unsafe fn inline_slot(obj: *const ObjectHeader, slot: usize) -> f64 {
    *((obj as *const u8).add(std::mem::size_of::<ObjectHeader>() + slot * 8) as *const f64)
}

/// The key's content bits for the read stub, or `None` when it has none (a
/// heap key past the inline length, or non-ASCII).
///
/// # Safety
/// `key_bits` is a string value.
#[inline(always)]
unsafe fn stub_key_bits(key_bits: u64) -> Option<u64> {
    if key_bits >> 48 == 0x7FF9 {
        return Some(key_bits);
    }
    super::read_stub::read_stub_key_bits(
        (key_bits & crate::value::POINTER_MASK) as *const crate::StringHeader,
    )
}

/// The shape answer (module docs, 1 and 2), or `None` for the generic Get.
///
/// `obj_box` is the receiver as the program holds it; only a POINTER-tagged
/// value above the handle floor is read, exactly the receivers whose `+4` word
/// the emitted read compares as a ShapeId (rule 3: no other cell kind can hold
/// a ShapeId there).
///
/// # Safety
/// `obj_box` and `key_bits` are live values.
#[inline]
pub(crate) unsafe fn shape_answer(obj_box: u64, key_bits: u64) -> Option<f64> {
    if obj_box >> 48 != 0x7FFD {
        return None;
    }
    let addr = (obj_box & crate::value::POINTER_MASK) as usize;
    if addr < perry_abi::RECEIVER_HANDLE_FLOOR {
        return None;
    }
    let tag = key_bits >> 48;
    if tag != 0x7FFF && tag != 0x7FF9 {
        return None;
    }
    let obj = addr as *const ObjectHeader;
    let shape_id = (*obj).parent_class_id;
    if let Some((keys, bound)) =
        super::shapes::plain_positional_key_words(super::shapes::ordinary_dir_addr(), shape_id)
    {
        if bound > 0 {
            let words = keys.words();
            for i in 0..bound {
                if *words.add(i) == key_bits {
                    return Some(inline_slot(obj, i));
                }
            }
        }
    }
    let content = stub_key_bits(key_bits)?;
    super::read_stub::read_stub_lookup_or_absent(obj, content)
}

/// What the receiver's shapes answered before the generic Get.
enum Prepass {
    /// The receiver's own data slot held the property: the read's value.
    Value(f64),
    /// What a confirmed `undefined` may file: `(receiver token, key content
    /// bits, terminal ShapeId)`, all values, computed before the Get.
    Absent(u64, u64, u32),
    /// Nothing proved; the generic Get answers alone.
    Unknown,
}

/// The receiver's own-key verdict for `key_bits`, from one lookup of its
/// shape's key list (`read_holder::dynamic_own_or_absent`).
///
/// An own DATA hit is read straight from its slot and filed in the read stub
/// under the receiver's token, as the generic by-name lane files the own
/// slots it resolves (`get_field_by_name::prime_read_stub`): the next read of
/// the key on a receiver with this shape is answered by the stub, and the
/// lookup is not repeated by the generic Get. An absent verdict is returned
/// for the caller to file once the Get confirms it.
///
/// Only receivers the stub may answer for are asked ([`read_stub_token`]:
/// an object with a real class id, no descriptors, a live shape), and the
/// `process.env` object, whose reads have their own semantics, is refused.
///
/// # Safety
/// As [`shape_answer`].
///
/// [`read_stub_token`]: super::read_stub::read_stub_token
unsafe fn own_or_absent(obj_box: u64, key_bits: u64) -> Prepass {
    use super::method_site::read_holder::{dynamic_own_or_absent, DynamicKeyVerdict};
    if obj_box >> 48 != 0x7FFD || key_bits >> 48 != 0x7FFF && key_bits >> 48 != 0x7FF9 {
        return Prepass::Unknown;
    }
    let Some(content) = stub_key_bits(key_bits) else {
        return Prepass::Unknown;
    };
    let addr = (obj_box & crate::value::POINTER_MASK) as usize;
    let obj = addr as *const ObjectHeader;
    let Some(token) = super::read_stub::read_stub_token(obj) else {
        return Prepass::Unknown;
    };
    if crate::process::is_process_env_ptr(addr) {
        return Prepass::Unknown;
    }
    let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(name) =
        crate::string::js_string_key_bytes(crate::JSValue::from_bits(content), &mut buf)
    else {
        return Prepass::Unknown;
    };
    match dynamic_own_or_absent(obj, name) {
        Some(DynamicKeyVerdict::Own { slot, live }) => {
            let value = super::field_get_set::object_field_at_with_live(obj, slot, live);
            // A tombstoned slot (#9064) continues the lookup past the
            // receiver, and an `undefined` read under a native-handle alias
            // is the generic entry's to resolve.
            if value.bits() == crate::value::TAG_HOLE
                || (value.is_undefined() && super::native_this_alias::alias_active())
            {
                return Prepass::Unknown;
            }
            let slot_word = if slot < live {
                slot
            } else {
                slot | crate::proxy::IC_SLOT_OVERFLOW_BIT
            };
            super::read_stub::read_stub_prime(obj, content, slot_word);
            Prepass::Value(f64::from_bits(value.bits()))
        }
        Some(DynamicKeyVerdict::Absent(terminal)) => Prepass::Absent(token, content, terminal),
        None => Prepass::Unknown,
    }
}

/// `o[k]` for a string `k` (the computed-read lowering's string arm): the
/// shape answer, else the generic by-value read
/// (`typed_feedback::js_typed_feedback_object_get_field_by_value_f64`, which
/// this takes the same `site_id`, `obj` and `key` for).
///
/// `obj_box` is the receiver's NaN-boxed value, which the shape answer reads;
/// `obj` is the handle the generic entry takes.
#[no_mangle]
pub extern "C" fn js_typed_feedback_object_get_field_by_key_f64(
    site_id: u64,
    obj: *const ObjectHeader,
    key: f64,
    obj_box: f64,
) -> f64 {
    let (obj_bits, key_bits) = (obj_box.to_bits(), key.to_bits());
    unsafe {
        if let Some(v) = shape_answer(obj_bits, key_bits) {
            return v;
        }
    }
    let prepass = unsafe { own_or_absent(obj_bits, key_bits) };
    if let Prepass::Value(v) = prepass {
        return v;
    }
    let value =
        crate::typed_feedback::js_typed_feedback_object_get_field_by_value_f64(site_id, obj, key);
    if let Prepass::Absent(token, content, terminal) = prepass {
        if value.to_bits() == crate::value::TAG_UNDEFINED {
            super::read_stub::read_stub_prime_absent(token, content, terminal);
        }
    }
    value
}

#[cfg(test)]
#[path = "dynamic_key_read_tests.rs"]
mod tests;
