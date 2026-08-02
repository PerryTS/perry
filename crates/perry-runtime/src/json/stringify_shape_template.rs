//! Homogeneous-array shape templates for `JSON.stringify`.
//!
//! When every element of an array shares the same `keys_array` pointer (same
//! shape), the key portion of each field can be pre-formatted once and reused
//! across every element. This module owns that template: its cache
//! (`SHAPE_CACHE`), the builder, and the per-element emitter. Split out of
//! `stringify.rs` to keep that file under the 2000-line lint gate.
//!
//! The single invariant everything here turns on: `keys_len` is the LOGICAL
//! property count and `field_count` is PHYSICAL (capped at the object's inline
//! slot allocation). Slots at or above `max(field_count, INLINE_SLOT_FLOOR)`
//! live in overflow storage, so they must be read through
//! `js_object_get_field`, never off the inline `fields_ptr` (#7264).

use super::stringify::{
    arm_to_json_result_guard, is_closure_value, is_symbol_value, object_get_to_json,
    stringify_value_depth,
};
use super::*;
use crate::{JSValue, StringHeader};

/// Cached shape template for a homogeneous array of objects.
pub(crate) struct ShapeTemplate {
    pub(crate) keys_arr: *mut crate::ArrayHeader,
    pub(crate) prefixes: Vec<String>,
    pub(crate) shape_fields: u32,
    /// True when element 0's fields are all primitives (no POINTER_TAG /
    /// UNDEFINED). Lets the emit path skip its per-element pre-scan.
    pub(crate) primitive_only: bool,
}

/// Look up (or build & insert) the shape template for an object. Returns
/// `None` if the object isn't templatable (no keys array, too many fields,
/// malformed key strings) or if the cache is full and missed.
///
/// Returns a raw pointer because lifetimes can't survive the TLS borrow.
/// The pointer stays valid until the next `take_shape_cache` (top-level
/// entry/exit) — within one stringify traversal we only `push`, and
/// `Box`'s heap address is stable across `Vec` growth.
#[inline]
pub(crate) unsafe fn shape_template_for(obj_ptr: *const u8) -> Option<*const ShapeTemplate> {
    let obj = obj_ptr as *const crate::ObjectHeader;
    let keys_arr = (*obj).keys_array;
    if keys_arr.is_null() {
        return None;
    }

    SHAPE_CACHE.with(|c| {
        // Fast path: linear scan from the back — recently-used entries
        // cluster there for typical traversal orders (shape A's elements
        // recurse into shape B repeatedly).
        {
            let cache = c.borrow();
            for entry in cache.iter().rev() {
                if entry.0 == keys_arr {
                    return Some(&*entry.1 as *const ShapeTemplate);
                }
            }
            if cache.len() >= SHAPE_CACHE_CAP {
                return None;
            }
        }

        // Miss — build, insert, return raw pointer to the boxed template.
        let elem_bits = make_pointer_bits(obj_ptr);
        let template = build_shape_prefix_template(elem_bits)?;
        let mut cache = c.borrow_mut();
        // Re-check cap after the borrow round-trip (a recursive call
        // during template build could have filled the cache).
        if cache.len() >= SHAPE_CACHE_CAP {
            return None;
        }
        cache.push((keys_arr, Box::new(template)));
        Some(&*cache.last().unwrap().1 as *const ShapeTemplate)
    })
}

/// Build a per-shape key-prefix template for a homogeneous array of objects.
///
/// When every element of an array shares the same `keys_array` pointer (same
/// shape), we can pre-format the key portion of each field once and reuse it
/// across every element — turning the per-field key lookup (load key f64,
/// extract pointer, `str_from_header`, 3 `push`/`push_str` calls) into a
/// single `push_str` of a cached prefix.
///
/// Prefix layout for N fields with keys k0..kN-1:
///   `prefixes[0]   = "{\"k0\":"`        (opening brace fused with first key)
///   `prefixes[f>0] = ",\"kf\":"`        (comma fused with key)
/// Close with `}`. This compresses ~7 per-field write ops down to ~2.
///
/// Returns `None` when the first element isn't a regular object, the keys
/// array is invalid, or any key string is malformed — callers fall back to
/// the generic slow path in that case.
pub(crate) unsafe fn build_shape_prefix_template(first_elem_bits: u64) -> Option<ShapeTemplate> {
    let tag = first_elem_bits & 0xFFFF_0000_0000_0000;
    let first_ptr = if tag == POINTER_TAG {
        (first_elem_bits & POINTER_MASK) as *const u8
    } else if is_raw_pointer(first_elem_bits) {
        first_elem_bits as *const u8
    } else {
        return None;
    };
    // Issue #639: Buffer / Uint8Array have no GcHeader, so `gc_obj_type`
    // would read 8 bytes before the BufferHeader (unrelated memory) and
    // could randomly return GC_TYPE_OBJECT. Bail to the per-element
    // slow path which dispatches via `is_registered_buffer`.
    if crate::buffer::is_registered_buffer(first_ptr as usize) {
        return None;
    }
    if gc_obj_type(first_ptr) != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    let obj = first_ptr as *const crate::ObjectHeader;
    // A non-zero `class_id` means this object may resolve `toJSON` (or other
    // serialization-affecting methods) on its prototype / class vtable — which
    // the prefix-template emit path can't see (it only inspects own fields).
    // Bail to the per-element slow path (`stringify_object_inner`), which
    // probes the prototype chain via `object_get_to_json`. Plain data object
    // literals and `JSON.parse` output carry `class_id == 0`, so the
    // array-of-objects fast path is unaffected for them. (#321 — a homogeneous
    // array of `class { toJSON() {…} }` instances must honour the prototype
    // `toJSON`.)
    if (*obj).class_id != 0 {
        return None;
    }
    // #6519: a URL instance is a class_id-0 object but must serialize as its
    // `href` string (handled by `stringify_object_inner`'s URL branch), never
    // as a templated dump of its 12 internal fields (which would also walk the
    // `searchParams` back-reference and throw). Bail so an array whose first
    // element is a URL routes every element through the per-element slow path.
    if crate::url::is_url_object_shape(obj as *mut crate::ObjectHeader) {
        return None;
    }
    let keys_arr = (*obj).keys_array;
    if keys_arr.is_null() {
        return None;
    }
    // #2438: array-index keys must enumerate first in ascending numeric order,
    // which the insertion-ordered prefix template can't express. Bail to the
    // generic slow path (`stringify_object_inner`), which reorders per spec.
    if crate::object::keys_contain_array_index(keys_arr) {
        return None;
    }
    // `keys_len` is authoritative — it is the LOGICAL property count, and the
    // only field that agrees with `Object.keys` / the slow object path.
    // `field_count` is a PHYSICAL count: it never exceeds the object's inline
    // slot allocation, so it under-counts any object grown past
    // `INLINE_SLOT_FLOOR` by name (`js_object_set_field_by_name` parks slots
    // ≥ `max(field_count, INLINE_SLOT_FLOOR)` in overflow storage and leaves
    // `field_count` pinned at the floor). `JSON.parse`'s tape materializer is
    // exactly that shape: it allocates with `js_object_alloc(0, 0)` and adds
    // every property by name, so a 6-key record reports `field_count == 4`.
    //
    // The old `min(keys_len, field_count)` therefore truncated EVERY element of
    // a homogeneous array to the first 4 properties with no diagnostic — silent
    // data loss in `JSON.stringify(JSON.parse(x))` (#7264). Latent since the
    // template landed (v0.5.65); exposed for ordinary 5–8-field records when
    // #6712 lowered `INLINE_SLOT_FLOOR` from 8 to 4.
    //
    // `min` was never needed for the opposite skew either: a pre-sized object
    // (`js_object_alloc(0, 8)` holding 2 real keys) has `field_count > keys_len`,
    // and `keys_len` already stops at the last real key. Slots at or above the
    // inline limit are read through `template_field_bits`, which routes them to
    // `js_object_get_field`'s overflow fallback.
    let shape_fields = (*keys_arr).length;
    if shape_fields == 0 || shape_fields > 32 {
        return None;
    }

    let keys_elements =
        (keys_arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let mut prefixes: Vec<String> = Vec::with_capacity(shape_fields as usize);
    for f in 0..shape_fields {
        let key_bits = (*keys_elements.add(f as usize)).to_bits();
        let key_tag = key_bits & 0xFFFF_0000_0000_0000;
        let key_ptr = if key_tag == STRING_TAG || key_tag == POINTER_TAG {
            (key_bits & POINTER_MASK) as *const StringHeader
        } else {
            key_bits as *const StringHeader
        };
        let key_str = str_from_header(key_ptr)?;
        let needs_escape = key_str.bytes().any(|b| b == b'"' || b == b'\\' || b < 0x20);
        let mut prefix = String::with_capacity(key_str.len() + 4);
        prefix.push(if f == 0 { '{' } else { ',' });
        if needs_escape {
            write_escaped_string(&mut prefix, key_str);
        } else {
            prefix.push('"');
            prefix.push_str(key_str);
            prefix.push('"');
        }
        prefix.push(':');
        prefixes.push(prefix);
    }

    // Sample first element to decide whether every field slot is already
    // a primitive (number/bool/null/string). When true, per-element emit
    // can skip the undefined/closure pre-scan.
    let mut primitive_only = true;
    for f in 0..shape_fields {
        let fb = template_field_bits(obj, f);
        if fb == TAG_UNDEFINED || (fb & 0xFFFF_0000_0000_0000) == POINTER_TAG {
            primitive_only = false;
            break;
        }
    }

    Some(ShapeTemplate {
        keys_arr,
        prefixes,
        shape_fields,
        primitive_only,
    })
}

/// Physical inline-slot limit of `obj` — the number of field slots the
/// allocator actually reserved. Mirrors `stringify_object_inner`'s
/// `alloc_limit` and every other access path (`object/alloc.rs`); slots at or
/// above it live in overflow storage, not in the inline region.
#[inline]
unsafe fn object_alloc_limit(obj: *const crate::ObjectHeader) -> u32 {
    std::cmp::max((*obj).field_count, crate::object::INLINE_SLOT_FLOOR as u32)
}

/// Read shape-template field slot `f` of `obj`: inline when it fits in the
/// physical allocation, overflow storage otherwise.
///
/// Reading `fields_ptr[f]` unconditionally is only correct while
/// `shape_fields <= alloc_limit`. It is not: a `JSON.parse` tape object is
/// allocated with `field_count = 0` and grown by name, so a 6-key record has
/// 4 inline slots and 2 overflow slots. Blind inline reads dropped those two
/// on every element of an array (#7264); worse, reading past the allocation
/// would walk into the adjacent arena object.
#[inline]
unsafe fn template_field_bits(obj: *const crate::ObjectHeader, f: u32) -> u64 {
    if f < object_alloc_limit(obj) {
        let fields_ptr =
            (obj as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;
        (*fields_ptr.add(f as usize)).to_bits()
    } else {
        crate::object::js_object_get_field(obj, f).bits()
    }
}

/// Record field `f`'s property name (from the shape template's shared
/// `keys_arr`) as the pending `toJSON` key before recursing into that field
/// (#5909). Mirrors the key decode in `build_shape_prefix_template`.
#[inline]
unsafe fn set_to_json_key_for_template_field(template: &ShapeTemplate, f: usize) {
    let keys_elements = (template.keys_arr as *const u8)
        .add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let key_bits = (*keys_elements.add(f)).to_bits();
    let key_tag = key_bits & 0xFFFF_0000_0000_0000;
    let key_ptr = if key_tag == STRING_TAG || key_tag == POINTER_TAG {
        (key_bits & POINTER_MASK) as *const StringHeader
    } else {
        key_bits as *const StringHeader
    };
    set_to_json_key_str(str_from_header(key_ptr).unwrap_or(""));
}

/// Fast emission path for an object element that matches the cached shape
/// template. Returns `true` when the element was emitted via the template;
/// `false` when the element diverges (different shape, skippable field, or
/// has a `toJSON` that must produce the replacement value). On `false` the
/// buffer is unchanged — the caller is responsible for falling back.
pub(crate) unsafe fn try_emit_shape_element(
    elem_bits: u64,
    template: &ShapeTemplate,
    buf: &mut String,
    depth: u32,
) -> bool {
    let tag = elem_bits & 0xFFFF_0000_0000_0000;
    let elem_ptr = if tag == POINTER_TAG {
        (elem_bits & POINTER_MASK) as *const u8
    } else if is_raw_pointer(elem_bits) {
        elem_bits as *const u8
    } else {
        return false;
    };
    if gc_obj_type(elem_ptr) != crate::gc::GC_TYPE_OBJECT {
        return false;
    }
    let obj = elem_ptr as *const crate::ObjectHeader;
    if (*obj).keys_array != template.keys_arr {
        return false;
    }

    let fields_ptr =
        (elem_ptr as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;
    let shape_fields = template.shape_fields;
    let prefixes = template.prefixes.as_slice();
    // Per ELEMENT, not per template: two objects can share a `keys_array`
    // (same shape) while one was pre-sized and the other grown by name, so the
    // inline/overflow split has to be re-derived from this element's own
    // header. Hoisted out of the emit loops — the common case is
    // `shape_fields <= alloc_limit` and the branch never taken.
    let alloc_limit = object_alloc_limit(obj);
    let field_bits_at = |f: usize| -> u64 {
        if (f as u32) < alloc_limit {
            (*fields_ptr.add(f)).to_bits()
        } else {
            crate::object::js_object_get_field(obj, f as u32).bits()
        }
    };

    // Primitive-only fast path (common case for JSON.parse output): skip
    // the undefined/closure pre-scan and trust that the sampled element 0
    // was representative. The emit loop handles stray POINTER_TAG values
    // via `stringify_value_depth`; a stray UNDEFINED / closure / symbol is
    // rare enough that we save `buf.len()` pre-emit and roll back on
    // detection.
    if template.primitive_only {
        let save_pos = buf.len();
        for f in 0..shape_fields as usize {
            let fb = field_bits_at(f);
            let field_val = f64::from_bits(fb);
            // UNDEFINED desyncs comma placement → roll back and let the
            // slow object path emit this element correctly.
            if fb == TAG_UNDEFINED {
                buf.truncate(save_pos);
                return false;
            }
            // Same for a function- or symbol-valued property: SerializeJSONObject
            // OMITS the key entirely, and this loop would emit `"k":null` (the
            // `stringify_value_depth` rendering of a closure) with the key
            // prefix already written. Element 0 having been all-primitives says
            // nothing about element N — the general path below pre-scans for
            // exactly this and bails; the fast path did not, so a later element
            // that took a function value rendered a spurious `null` member.
            if (fb & 0xFFFF_0000_0000_0000) == POINTER_TAG
                && (is_closure_value(fb) || is_symbol_value(fb))
            {
                buf.truncate(save_pos);
                return false;
            }
            buf.push_str(&prefixes[f]);
            let vtag = fb & 0xFFFF_0000_0000_0000;
            if fb == TAG_NULL {
                buf.push_str("null");
            } else if fb == TAG_TRUE {
                buf.push_str("true");
            } else if fb == TAG_FALSE {
                buf.push_str("false");
            } else if vtag == STRING_TAG {
                let str_ptr = (fb & POINTER_MASK) as *const StringHeader;
                if let Some(s) = str_from_header(str_ptr) {
                    write_escaped_string(buf, s);
                } else {
                    buf.push_str("null");
                }
            } else if vtag == crate::value::SHORT_STRING_TAG {
                let jsval = JSValue::from_bits(fb);
                let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                let n = jsval.short_string_to_buf(&mut scratch);
                if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
                    write_escaped_string(buf, s);
                } else {
                    buf.push_str("null");
                }
            } else if vtag == POINTER_TAG || is_raw_pointer(fb) {
                set_to_json_key_for_template_field(template, f);
                stringify_value_depth(field_val, TYPE_UNKNOWN, buf, depth + 1);
            } else {
                // A BigInt field reaches `serialize_bigint` via `write_number`,
                // which reads the pending `toJSON` key — record it first (#5909).
                if vtag == BIGINT_TAG {
                    set_to_json_key_for_template_field(template, f);
                }
                write_number(buf, field_val);
            }
        }
        buf.push('}');
        return true;
    }

    // General path: template contains (or may contain) pointer/undefined
    // fields. Pre-scan to honor JSON spec (skip undefined, skip closures,
    // respect toJSON).
    let mut has_pointer_fields = false;
    for f in 0..shape_fields as usize {
        let fb = field_bits_at(f);
        if fb == TAG_UNDEFINED {
            return false;
        }
        if (fb & 0xFFFF_0000_0000_0000) == POINTER_TAG {
            has_pointer_fields = true;
            if is_closure_value(fb) || is_symbol_value(fb) {
                return false;
            }
        }
    }
    if has_pointer_fields {
        if let Some(to_json_val) = object_get_to_json(elem_ptr) {
            arm_to_json_result_guard(to_json_val);
            stringify_value_depth(to_json_val, TYPE_UNKNOWN, buf, depth + 1);
            SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
            return true;
        }
    }
    for f in 0..shape_fields as usize {
        buf.push_str(&prefixes[f]);
        let fb = field_bits_at(f);
        let field_val = f64::from_bits(fb);
        let vtag = fb & 0xFFFF_0000_0000_0000;
        if fb == TAG_NULL {
            buf.push_str("null");
        } else if fb == TAG_TRUE {
            buf.push_str("true");
        } else if fb == TAG_FALSE {
            buf.push_str("false");
        } else if vtag == STRING_TAG {
            let str_ptr = (fb & POINTER_MASK) as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if vtag == crate::value::SHORT_STRING_TAG {
            let jsval = JSValue::from_bits(fb);
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let n = jsval.short_string_to_buf(&mut scratch);
            if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if vtag == POINTER_TAG || is_raw_pointer(fb) {
            set_to_json_key_for_template_field(template, f);
            stringify_value_depth(field_val, TYPE_UNKNOWN, buf, depth + 1);
        } else {
            // A BigInt field reaches `serialize_bigint` via `write_number`,
            // which reads the pending `toJSON` key — record it first (#5909).
            if vtag == BIGINT_TAG {
                set_to_json_key_for_template_field(template, f);
            }
            write_number(buf, field_val);
        }
    }
    buf.push('}');
    true
}
