//! Ordinary RegExp births. The memo holds only a validated ShapeId; the
//! shape carries the private matcher, lastIndex attributes and prototype.
use super::{RegExpData, RegExpHeader};
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::value::js_nanbox_pointer;
use std::cell::Cell;

crate::perry_thread_local! {
    static PROTOTYPE_READ: crate::object::IntrinsicPrivateReadSite =
        const { crate::object::IntrinsicPrivateReadSite::new("[[RegExpPrototype]]") };
    static BIRTH_SHAPE: Cell<u32> = const { Cell::new(0) };
}

pub(super) fn new(scope: &RuntimeHandleScope, data: &RuntimeHandle<'_>) -> *mut RegExpHeader {
    let shape = BIRTH_SHAPE.with(Cell::get);
    let receiver = scope.root_raw_mut_ptr(crate::object::object_alloc_plain_born(2, shape));
    let cached = receiver.with_const_ptr::<RegExpHeader, _>(|r| unsafe {
        crate::object::shapes::object_shape_stamp(r) == shape
    });
    if !cached {
        prepare_shape(scope, &receiver);
    }
    receiver.with_mut_ptr::<RegExpHeader, _>(|r| unsafe {
        crate::object::store_object_field_slot_layout_deferred(
            r,
            0,
            data.with_const_ptr::<RegExpData, _>(|d| js_nanbox_pointer(d as i64).to_bits()),
        );
        crate::object::store_object_field_slot_layout_deferred(r, 1, 0.0f64.to_bits());
    });
    receiver.with_mut_ptr::<RegExpHeader, _>(|r| r)
}

#[cold]
#[inline(never)]
fn prepare_shape(scope: &RuntimeHandleScope, receiver: &RuntimeHandle<'_>) {
    use crate::object::canonical_keys::{CanonicalKeys, SharedLayout};
    use crate::object::key_attrs::{attr_bits_to_entry, PRIVATE_FIELD_ENTRY};
    let prototype = scope.root_raw_mut_ptr(
        crate::value::js_nanbox_get_pointer(intrinsic_prototype()) as *mut RegExpHeader,
    );
    let proto_id = prototype
        .with_mut_ptr::<RegExpHeader, _>(|p| unsafe {
            crate::object::proto_validity::mark_object_as_prototype(p as usize)
        })
        .expect("intrinsic prototype has a stable identity");
    let private_key = super::MATCHER_READ.with(|site| site.birth_key());
    let proof = SharedLayout::shape_cache_entry();
    let keys = unsafe {
        crate::object::canonical_keys::extend_key_with_entry(
            &proof,
            CanonicalKeys::EMPTY,
            private_key,
            PRIVATE_FIELD_ENTRY,
        )
    };
    let keys = scope.root_raw_mut_ptr(keys.as_ptr());
    let index_key = crate::string::intern_ascii_literal(b"lastIndex");
    let final_keys = keys.with_mut_ptr::<crate::array::ArrayHeader, _>(|keys| unsafe {
        crate::object::canonical_keys::extend_key_with_entry(
            &proof,
            CanonicalKeys::from_rooted(keys, 1),
            index_key,
            attr_bits_to_entry(1),
        )
    });
    // No collecting operation between the final canonical keys and their
    // publication into the rooted receiver's shape.
    receiver.with_mut_ptr::<RegExpHeader, _>(|r| unsafe {
        assert!(crate::object::shapes::stamp_linked_final_shape(
            r,
            final_keys.as_ptr(),
            2,
            proto_id,
            prototype.with_const_ptr::<RegExpHeader, _>(|p| js_nanbox_pointer(p as i64).to_bits()),
            |_| false,
        ));
        let shape = crate::object::shapes::object_shape_stamp(r);
        BIRTH_SHAPE.with(|memo| memo.set(shape));
        // The canonical keys above prove both namespaces and positions.
        // Prime the existing generic read sites from those birth facts so the
        // first access does not rediscover the layout we just constructed.
        super::MATCHER_READ.with(|site| site.prime_birth(shape, 0, 2));
        super::LAST_INDEX_READ.with(|site| site.prime_own_inline(shape, 1, 2));
    });
}

// Read the realm intrinsic through the same private slot machinery as matcher
// data. Neither this site nor BIRTH_SHAPE retains a managed pointer.
pub(crate) fn intrinsic_prototype() -> f64 {
    let global = crate::object::js_get_global_this();
    PROTOTYPE_READ
        .with(|site| site.read(global))
        .expect("realm has installed the RegExp intrinsic")
}
