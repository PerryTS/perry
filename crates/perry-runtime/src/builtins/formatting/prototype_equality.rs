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
        // #4937: a non-zero class_id is only a constructor marker when it is
        // registered in CLASS_NAMES (the same test class_decl_prototype_value
        // uses). Object literals carry an unregistered layout-shape id there,
        // while a `{}`-born object mutated afterwards keeps class_id 0 — both
        // have Object.prototype, so normalize unregistered ids to 0 or the
        // two would spuriously compare as prototype-different.
        //
        // The test is REGISTERED, not "registered under a non-empty name": an
        // anonymous class expression (`const M = Mixin(Base)`, issue #5952)
        // registers the empty string as its `.name` and is every bit a real
        // constructor, so its instances must keep their own prototype identity
        // rather than collapse onto Object.prototype.
        let class_id = (*obj).class_id;
        let proto_class_id =
            if class_id != 0 && crate::object::class_name_for_id(class_id).is_none() {
                0
            } else {
                class_id
            };
        Some(CLASS_PROTO_NAMESPACE | proto_class_id as u64)
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

#[cfg(test)]
mod tests {
    /// #4937: codegen gives object literals an unregistered layout-shape id in
    /// `class_id`, while a `{}`-born object mutated afterwards keeps 0. Both
    /// have Object.prototype — the prototype gate must not separate them.
    /// A class_id registered in CLASS_NAMES still marks a real constructor.
    #[test]
    fn unregistered_shape_id_normalizes_to_plain_object_prototype() {
        unsafe {
            let key = crate::string::js_string_from_bytes(b"x".as_ptr(), 1);

            // "literal": unregistered non-zero shape id.
            let lit = crate::object::js_object_alloc(424_242, 1);
            crate::object::js_object_set_field_by_name(lit, key, 1.0);
            let lit_v = crate::value::js_nanbox_pointer(lit as i64);

            // "{} then mutated": class_id 0.
            let dyn_obj = crate::object::js_object_alloc(0, 0);
            crate::object::js_object_set_field_by_name(dyn_obj, key, 1.0);
            let dyn_v = crate::value::js_nanbox_pointer(dyn_obj as i64);

            // registered class instance with the same body.
            let name = b"ProtoEqTestClass4937";
            crate::object::js_register_class_name(424_243, name.as_ptr(), name.len() as u32);
            let inst = crate::object::js_object_alloc(424_243, 1);
            crate::object::js_object_set_field_by_name(inst, key, 1.0);
            let inst_v = crate::value::js_nanbox_pointer(inst as i64);

            assert!(!super::prototypes_differ(lit_v, dyn_v));
            assert!(super::prototypes_differ(inst_v, lit_v));
            assert!(super::prototypes_differ(inst_v, dyn_v));

            assert_eq!(
                crate::value::js_is_truthy(crate::builtins::js_util_is_deep_strict_equal(
                    lit_v, dyn_v
                )),
                1
            );
            assert_eq!(
                crate::value::js_is_truthy(crate::builtins::js_util_is_deep_strict_equal(
                    inst_v, dyn_v
                )),
                0
            );
        }
    }
}
