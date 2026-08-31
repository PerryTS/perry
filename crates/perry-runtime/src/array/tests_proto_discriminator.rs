//! Unit tests — Array.prototype method discrimination (split from `tests.rs`
//! for the 2,000-line file gate; `use super::*` reaches the shared helpers).

use super::*;

#[test]
fn array_prototype_method_discriminator_separates_foreign_builtins() {
    // Reads realm intrinsics and holds raw pointers across allocating calls,
    // and libtest gives each test its own thread where `GLOBAL_THIS_PTR` can be
    // re-created — so resolve the whole snapshot inside one iteration and retry
    // until a GC-quiet pass yields a self-consistent view. Mirrors
    // `array_literal_shares_the_realm_array_prototype`'s loop above.
    let mut checked = false;
    for _ in 0..256 {
        let global = crate::object::js_get_global_this();
        let global_ptr =
            crate::value::js_nanbox_get_pointer(global) as *const crate::object::ObjectHeader;
        if global_ptr.is_null() {
            std::thread::yield_now();
            continue;
        }

        let proto_of = |ctor_name: &[u8]| -> Option<f64> {
            let ctor =
                crate::object::js_object_get_field_by_name(global_ptr, string_key(ctor_name));
            if !ctor.is_pointer() {
                return None;
            }
            let ctor_ptr =
                crate::value::js_nanbox_get_pointer(f64::from_bits(ctor.bits())) as usize;
            if ctor_ptr == 0 {
                return None;
            }
            Some(crate::closure::closure_get_dynamic_prop(
                ctor_ptr,
                "prototype",
            ))
        };

        let (Some(array_proto), Some(string_proto)) = (proto_of(b"Array"), proto_of(b"String"))
        else {
            std::thread::yield_now();
            continue;
        };
        let array_proto_ptr =
            crate::value::js_nanbox_get_pointer(array_proto) as *const crate::object::ObjectHeader;
        let string_proto_ptr =
            crate::value::js_nanbox_get_pointer(string_proto) as *const crate::object::ObjectHeader;
        if array_proto_ptr.is_null() || string_proto_ptr.is_null() {
            std::thread::yield_now();
            continue;
        }

        let array_concat =
            crate::object::js_object_get_field_by_name_f64(array_proto_ptr, string_key(b"concat"));
        let string_concat =
            crate::object::js_object_get_field_by_name_f64(string_proto_ptr, string_key(b"concat"));
        if !crate::value::JSValue::from_bits(array_concat.to_bits()).is_pointer()
            || !crate::value::JSValue::from_bits(string_concat.to_bits()).is_pointer()
        {
            std::thread::yield_now();
            continue;
        }

        // The Array borrow must still be claimed — this is the behavior the
        // original classification existed to protect (`obj.concat =
        // Array.prototype.concat` has to run the array engine on `obj`).
        assert!(
            crate::object::is_array_prototype_method_value(array_concat, "concat"),
            "Array.prototype.concat must be recognized as an Array builtin"
        );
        // The foreign borrow must NOT be claimed.
        assert!(
            !crate::object::is_array_prototype_method_value(string_concat, "concat"),
            "String.prototype.concat must not be mistaken for an Array builtin"
        );
        // Right closure, wrong method name is also a mismatch — the predicate
        // keys on the (method, closure) pair, not on "is some Array builtin".
        assert!(
            !crate::object::is_array_prototype_method_value(array_concat, "push"),
            "Array.prototype.concat must not answer for `push`"
        );
        // Non-callable / non-pointer slots are never a borrowed builtin.
        assert!(!crate::object::is_array_prototype_method_value(
            1.0, "concat"
        ));
        assert!(!crate::object::is_array_prototype_method_value(
            f64::from_bits(crate::value::TAG_UNDEFINED),
            "concat"
        ));

        checked = true;
        break;
    }
    assert!(
        checked,
        "never obtained a GC-quiet view of Array.prototype / String.prototype"
    );
}
