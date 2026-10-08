//! Instance ancestry follows the prototype identities in the live shapes.
//! Declaration metadata only supplies an absent lazy holder and generic origin.
use crate::object::{shapes, ObjectHeader};

/// Parameter proofs use `build = false`: absent lazy holders decline the
/// optimization without allocating or running JavaScript.
pub(crate) unsafe fn class_shape_reaches(
    object: *const ObjectHeader,
    want: u32,
    build: bool,
) -> bool {
    if object.is_null() || want == 0 {
        return false;
    }
    let record = shapes::object_shape_record(object);
    // Exact identity and null are complete proofs even for a specialization:
    // both sides of an equal CLASS identity resolve to the same live holder.
    // Only an unequal identity needs the generic-origin projection.
    if let Some(record) = record.as_ref() {
        let identity = record.proto_id();
        if identity == (shapes::PROTO_ID_CLASS | u64::from(want)) {
            return true;
        }
        if identity == shapes::PROTO_ID_NULL {
            return false;
        }
    }
    let want = crate::object::class_registry::decl_prototype_identity_id(want);
    // The immutable class identity itself proves the immediate holder, even
    // while the holder is lazy. This needs neither a root nor a materialization.
    if let Some(record) = record {
        let identity = record.proto_id();
        if identity == (shapes::PROTO_ID_CLASS | u64::from(want)) {
            return true;
        }
        if identity == shapes::PROTO_ID_NULL {
            return false;
        }
        if identity == shapes::PROTO_ID_DEFAULT {
            if want == 0xFFFF0050 {
                return true;
            }
            // DEFAULT steps to the realm's immutable-prototype exotic,
            // Object.prototype, whose next link is always null. As in the
            // read-holder walk, that terminal needs no scope or shape probe.
            if crate::array::object_prototype_addr_if_resolved() != 0 {
                return false;
            }
        }
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let current = scope.root_raw_mut_ptr(object as *mut ObjectHeader);
    // CLASS, DEFAULT and NULL identities answer without a builtin lookup.
    // Only an explicit physical link needs the target's concrete object.
    let mut builtin_target = None;
    for _ in 0..128 {
        let obj = current.get_raw_mut_ptr::<ObjectHeader>();
        let Some(header) = crate::value::addr_class::try_read_gc_header(obj as usize) else {
            return false;
        };
        if header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
            return false;
        }
        if header.obj_type != crate::gc::GC_TYPE_OBJECT {
            if !build {
                return false;
            }
            // Arrays and functions have their own shape/header layout. Their
            // existing prototype reader supplies the next physical hop.
            // Resolve a collecting target before reading the next raw link.
            let _ = physical_target(&scope, &mut builtin_target, want, build);
            let value = current
                .with_const_ptr(|p: *const ObjectHeader| crate::value::js_nanbox_pointer(p as i64));
            let next_value = crate::object::js_object_get_prototype_of(value);
            let next_value = crate::JSValue::from_bits(next_value.to_bits());
            if !next_value.is_pointer() {
                return false;
            }
            let next = next_value.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
            let target = physical_target(&scope, &mut builtin_target, want, build);
            if !target.is_null() && next == target {
                return true;
            }
            if next == current.get_raw_mut_ptr::<ObjectHeader>() {
                return false;
            }
            current.set_raw_mut_ptr(next);
            continue;
        }
        let Some(record) = shapes::object_shape_record(obj) else {
            return false;
        };
        let identity = record.proto_id();
        if (shapes::PROTO_ID_CLASS..shapes::PROTO_ID_MIXED).contains(&identity) {
            let cid = crate::object::class_registry::decl_prototype_identity_id(identity as u32);
            if cid == want {
                return true;
            }
            let mut next = crate::object::class_value::class_decl_prototype_link(cid);
            if next.is_null() && build {
                let v = crate::JSValue::from_bits(
                    crate::object::class_registry::class_decl_prototype_value(cid).to_bits(),
                );
                if v.is_pointer() {
                    next = v.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
                }
            }
            if next.is_null() {
                return false;
            }
            current.set_raw_mut_ptr(next);
            continue;
        }
        if identity == shapes::PROTO_ID_DEFAULT {
            if want == 0xFFFF0050 {
                return true;
            }
            if !build {
                return false;
            }
            let mut next = crate::array::object_prototype_addr_if_resolved() as *mut ObjectHeader;
            if next.is_null() {
                next = crate::array::object_prototype_addr() as *mut ObjectHeader;
            }
            if next.is_null() {
                return false;
            }
            if next == obj {
                return false;
            }
            current.set_raw_mut_ptr(next);
            continue;
        }
        if identity == shapes::PROTO_ID_NULL {
            return false;
        }
        let target = physical_target(&scope, &mut builtin_target, want, build);
        let obj = current.get_raw_mut_ptr::<ObjectHeader>();
        let bits = shapes::object_prototype_word(obj);
        let v = crate::JSValue::from_bits(bits);
        if !v.is_pointer() {
            return false;
        }
        let next = v.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
        if !target.is_null() && next == target {
            return true;
        }
        if next == obj {
            return false;
        }
        current.set_raw_mut_ptr(next);
    }
    false
}

/// A local root, resolved only when a physical prototype hop needs it.
unsafe fn physical_target<'scope>(
    scope: &'scope crate::gc::RuntimeHandleScope,
    target: &mut Option<crate::gc::RuntimeHandle<'scope>>,
    want: u32,
    build: bool,
) -> *mut ObjectHeader {
    if !build || want < 0xFFFF0000 {
        return crate::object::class_value::class_decl_prototype_link(want);
    }
    if target.is_none() {
        let name = super::static_dispatch::heap_builtin_name(want).or(match want {
            0xFFFF0024 => Some("Array"),
            0xFFFF0027 => Some("Promise"),
            0xFFFF002C => Some("WeakMap"),
            0xFFFF002D => Some("WeakSet"),
            0xFFFF0050 => Some("Object"),
            0xFFFF00D0 => Some("Number"),
            0xFFFF00D1 => Some("String"),
            0xFFFF00D2 => Some("Boolean"),
            0xFFFF00D3 => Some("BigInt"),
            0xFFFF00D4 => Some("Symbol"),
            _ => None,
        });
        let bits = name
            .map(|n| crate::object::builtin_prototype_value(n).to_bits())
            .or_else(|| crate::object::class_registry::reserved_native_parent_prototype_bits(want));
        *target = Some(
            scope.root_nanbox_f64(f64::from_bits(bits.unwrap_or(crate::value::TAG_UNDEFINED))),
        );
    }
    let value = crate::JSValue::from_bits(target.as_ref().unwrap().get_nanbox_f64().to_bits());
    if value.is_pointer() {
        value.as_pointer::<ObjectHeader>() as *mut ObjectHeader
    } else {
        std::ptr::null_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn class_shape_walk_observes_holder_and_receiver_relinks() {
        let _lock = crate::gc::global_side_table_test_lock();
        const BASE: u32 = 0x6D51;
        const CHILD: u32 = 0x6D52;
        const OTHER: u32 = 0x6D53;
        for (id, name) in [
            (BASE, b"AncestryBase".as_slice()),
            (CHILD, b"AncestryChild"),
            (OTHER, b"AncestryOther"),
        ] {
            unsafe {
                crate::object::js_register_class_name(id, name.as_ptr(), name.len() as u32);
            }
        }
        crate::object::js_register_class_parent(CHILD, BASE);
        let scope = crate::gc::RuntimeHandleScope::new();
        let receiver = scope.root_raw_mut_ptr(crate::object::js_object_alloc(CHILD, 0));
        assert!(receiver.with_const_ptr(|p| unsafe { class_shape_reaches(p, CHILD, false) }));
        assert!(receiver.with_const_ptr(|p| unsafe { class_shape_reaches(p, BASE, true) }));
        assert!(receiver.with_const_ptr(|p| unsafe { class_shape_reaches(p, BASE, false) }));
        let before = receiver.with_const_ptr(|p| unsafe { *(p as *const u64) });
        let child = scope.root_nanbox_f64(
            crate::object::class_registry::class_decl_prototype_value(CHILD),
        );
        let other = scope.root_nanbox_f64(
            crate::object::class_registry::class_decl_prototype_value(OTHER),
        );
        crate::object::js_object_set_prototype_of(child.get_nanbox_f64(), other.get_nanbox_f64());
        assert_eq!(
            before,
            receiver.with_const_ptr(|p| unsafe { *(p as *const u64) })
        );
        assert!(!receiver.with_const_ptr(|p| unsafe { class_shape_reaches(p, BASE, false) }));
        assert!(receiver.with_const_ptr(|p| unsafe { class_shape_reaches(p, OTHER, false) }));
        let value = receiver
            .with_const_ptr(|p: *const ObjectHeader| crate::value::js_nanbox_pointer(p as i64));
        crate::object::js_object_set_prototype_of(value, f64::from_bits(crate::value::TAG_NULL));
        assert!(!receiver.with_const_ptr(|p| unsafe { class_shape_reaches(p, CHILD, false) }));
    }
}
