//! Prototype comparison for `util.isDeepStrictEqual` / `assert.deepStrictEqual`
//! (issue #2934).
//!
//! Node's default deep-strict comparison is prototype-sensitive: two values
//! with identical own properties but different `[[Prototype]]` are NOT equal
//! (e.g. `{ x: 1 }` vs `Object.create(null)` with `x = 1`, or instances of two
//! different constructors). The shared helper used to fall back to formatting
//! the enumerable body, which dropped this distinction.
//!
//! `prototype_token` returns a comparable identity token for the operand's
//! prototype; the deep-equal helper short-circuits to `false` when two
//! heap-object operands have differing tokens before comparing their bodies.

/// Bits returned by `Object.setPrototypeOf(o, null)` / null-prototype objects.
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
/// POINTER_TAG nibble (top 16 bits) for NaN-boxed heap pointers.
const POINTER_TAG_TOP16: u64 = 0x7FFD;
/// Namespace bit so a class-id token (small u32) can never collide with a
/// recorded `setPrototypeOf` prototype value's raw bits or `TAG_NULL`.
const CLASS_PROTO_NAMESPACE: u64 = 0x9000_0000_0000_0000;

/// Resolve the raw heap address of an object operand, or `None` if the value is
/// not a tagged/raw heap object we model with a prototype.
fn heap_object_addr(value: f64) -> Option<usize> {
    let bits = value.to_bits();
    let top16 = bits >> 48;
    let addr = if top16 == POINTER_TAG_TOP16 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if top16 == 0x0000 {
        // Module-level object literals are stored as raw I64 pointers.
        bits as usize
    } else {
        return None;
    };
    if addr < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    unsafe {
        let gc_header =
            (addr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return None;
        }
    }
    Some(addr)
}

/// A comparable identity token for an operand's `[[Prototype]]`.
///
/// - `None` for non-object operands (primitives, collections, typed arrays,
///   arrays) — the caller only applies the prototype gate when BOTH operands
///   resolve to a token, so non-object shapes keep their existing handling.
/// - An explicit `Object.setPrototypeOf` value's bits when recorded.
/// - `TAG_NULL` for null-prototype objects.
/// - `CLASS_PROTO_NAMESPACE | class_id` otherwise (plain literals share
///   `class_id == 0` → `Object.prototype`; class instances carry their
///   constructor's class id).
pub(super) fn prototype_token(value: f64) -> Option<u64> {
    let addr = heap_object_addr(value)?;

    if let Some(proto_bits) = crate::object::prototype_chain::object_static_prototype(addr) {
        return Some(proto_bits);
    }

    unsafe {
        let gc_header =
            (addr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header)._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0 {
            return Some(TAG_NULL);
        }
        let obj = addr as *const crate::object::ObjectHeader;
        Some(CLASS_PROTO_NAMESPACE | (*obj).class_id as u64)
    }
}

/// Returns `true` when both operands are heap objects whose prototypes differ
/// (so they cannot be deep-strict-equal). Returns `false` when the gate does
/// not apply (one or both aren't prototype-bearing objects) or the prototypes
/// match.
pub(super) fn prototypes_differ(left: f64, right: f64) -> bool {
    match (prototype_token(left), prototype_token(right)) {
        (Some(l), Some(r)) => l != r,
        _ => false,
    }
}
