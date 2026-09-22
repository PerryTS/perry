//! Lane 1 of the single-path object model: the invariants the emitted read
//! path is allowed to depend on, asserted as runtime tests.
//!
//! Three rules, one file, because they are one argument:
//!
//! * **Rule 1** — every descriptor change changes the ShapeId of an ordinary
//!   object. A read that has compared the shape word has therefore also proved
//!   "no accessor, no non-default attribute" for every key of that shape, and
//!   needs no separate `OBJ_FLAG_HAS_DESCRIPTORS` test.
//! * **Rule 2** — ShapeIds are never reused, AND no other producer mints a u32
//!   inside the ShapeId range. The second half is what makes an UNSTAMPED
//!   object safe: its `+4` word holds `parent_class_id`, and the read path
//!   distinguishes the two by range alone ([`shapes::is_shape_id`]).
//! * **Rule 3** — no pointer-tagged non-object cell holds a value in the
//!   ShapeId range at payload `+4`, so a shape compare on a mis-typed receiver
//!   fails instead of aliasing a live shape. (Enforced at the allocation sites;
//!   the enumerating test lives in [`super::shape_rule3_tests`].)
//!
//! Each test states what it would catch, and asserts a FAILURE mode that the
//! code under test can actually produce — a shape compare that stays equal is
//! not "slow", it is a wrong value silently returned.

use super::descriptor_state::{
    clear_accessor_descriptor, clear_object_descriptors, clear_property_attrs,
    install_fresh_accessor_property, object_has_descriptors, set_accessor_descriptor,
    set_builtin_accessor_descriptor, set_builtin_property_attrs, set_property_attrs,
    set_property_attrs_batch, transfer_descriptor_owner, AccessorDescriptor, PropertyAttrs,
};
use super::{js_object_alloc, js_object_set_field_by_name, shapes, ObjectHeader};

/// An ordinary, shaped, stamped object carrying `keys` as plain data
/// properties — the receiver every rule-1 assertion is about.
unsafe fn shaped_object(keys: &[&str]) -> *mut ObjectHeader {
    let obj = js_object_alloc(0, 0);
    for (i, name) in keys.iter().enumerate() {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        js_object_set_field_by_name(obj, key, i as f64);
    }
    assert!(
        shapes::is_shape_id(shapes::object_shape_stamp(obj)),
        "test premise: a plain object with data properties must be stamped"
    );
    obj
}

/// Run `mutate` on a freshly built receiver and assert the ShapeId moved.
///
/// The `_ne` is the whole point: an install that leaves the shape equal is
/// invisible in program output (the descriptor still works, through the side
/// table) and is exactly the case that makes a shape-only read return the raw
/// slot for what is now an accessor.
fn assert_shape_moves(what: &str, mutate: impl FnOnce(usize)) {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = shaped_object(&["alpha", "beta"]);
        let before = shapes::object_shape_stamp(obj);
        mutate(obj as usize);
        let after = shapes::object_shape_stamp(obj);
        assert!(
            shapes::is_shape_id(after),
            "{what}: the receiver must still carry a real ShapeId, got {after:#x}"
        );
        assert_ne!(
            before, after,
            "{what}: a descriptor change left the ShapeId unchanged — a read that \
             only compares the shape word would serve the pre-descriptor answer"
        );
    }
}

fn accessor() -> AccessorDescriptor {
    // Non-zero halves so the #10287 keyed encoding sees a get+set accessor.
    // The bits are never dereferenced by any of these paths.
    AccessorDescriptor {
        get: crate::value::JSValue::pointer(0x1000 as *const u8).bits(),
        set: 0,
    }
}

const DEFAULT_ATTRS: PropertyAttrs = PropertyAttrs::new(true, true, true);
const FROZEN_ATTRS: PropertyAttrs = PropertyAttrs::new(false, true, false);

/// Keyless receivers expose the flag transition directly: no descriptor
/// install can incidentally mint a successor shape for these operations.
#[test]
fn rule1_integrity_flags_transition_keyless_shapes() {
    let _lock = crate::gc::global_side_table_test_lock();
    for (name, operation) in [
        (
            "preventExtensions",
            super::js_object_prevent_extensions as extern "C" fn(f64) -> f64,
        ),
        ("seal", super::js_object_seal),
        ("freeze", super::js_object_freeze),
    ] {
        unsafe {
            let obj = shaped_object(&[]);
            let sibling = shaped_object(&[]);
            let before = shapes::object_shape_stamp(obj);
            assert_eq!(before, shapes::object_shape_stamp(sibling));
            let value = crate::value::js_nanbox_pointer(obj as i64);
            operation(value);
            let after = shapes::object_shape_stamp(obj);
            assert_ne!(before, after, "{name} must retire the extensible shape");
            assert_eq!(
                before,
                shapes::object_shape_stamp(sibling),
                "{name} must not change a sibling's shape"
            );
            operation(value);
            assert_eq!(
                after,
                shapes::object_shape_stamp(obj),
                "repeated {name} must not mint another shape for unchanged flags"
            );
        }
    }
}

#[test]
fn rule1_set_property_attrs_transitions() {
    assert_shape_moves("set_property_attrs", |addr| {
        set_property_attrs(addr, "alpha".to_string(), FROZEN_ATTRS);
    });
}

#[test]
fn rule1_set_property_attrs_batch_transitions() {
    assert_shape_moves("set_property_attrs_batch", |addr| {
        set_property_attrs_batch(addr, &[("alpha", FROZEN_ATTRS), ("beta", FROZEN_ATTRS)]);
    });
}

#[test]
fn rule1_clear_property_attrs_transitions() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = shaped_object(&["alpha"]);
        let addr = obj as usize;
        set_property_attrs(addr, "alpha".to_string(), FROZEN_ATTRS);
        let before = shapes::object_shape_stamp(obj);
        clear_property_attrs(addr, "alpha");
        assert_ne!(
            before,
            shapes::object_shape_stamp(obj),
            "clear_property_attrs: removing a descriptor restores writability, \
             so it must retire every cache keyed on the old shape"
        );
    }
}

#[test]
fn rule1_set_accessor_descriptor_transitions() {
    assert_shape_moves("set_accessor_descriptor", |addr| {
        set_accessor_descriptor(addr, "alpha".to_string(), accessor());
    });
}

#[test]
fn rule1_install_fresh_accessor_property_transitions() {
    assert_shape_moves("install_fresh_accessor_property", |addr| {
        install_fresh_accessor_property(addr, "gamma".to_string(), accessor(), DEFAULT_ATTRS);
    });
}

#[test]
fn rule1_clear_accessor_descriptor_transitions() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = shaped_object(&["alpha"]);
        let addr = obj as usize;
        set_accessor_descriptor(addr, "alpha".to_string(), accessor());
        let before = shapes::object_shape_stamp(obj);
        clear_accessor_descriptor(addr, "alpha");
        assert_ne!(
            before,
            shapes::object_shape_stamp(obj),
            "clear_accessor_descriptor: the key becomes a plain data slot again"
        );
    }
}

#[test]
fn rule1_set_builtin_accessor_descriptor_transitions() {
    assert_shape_moves("set_builtin_accessor_descriptor", |addr| {
        set_builtin_accessor_descriptor(addr, "alpha".to_string(), accessor(), DEFAULT_ATTRS);
    });
}

#[test]
fn rule1_set_builtin_property_attrs_transitions() {
    assert_shape_moves("set_builtin_property_attrs", |addr| {
        set_builtin_property_attrs(addr, "alpha".to_string(), FROZEN_ATTRS);
    });
}

/// `clear_object_descriptors` is a BULK removal: after it, every key of the
/// receiver is an ordinary writable/enumerable/configurable data property
/// again. Before this lane it made no shape call at all, so a cache primed
/// while the object was frozen kept serving the frozen answer.
///
/// Its only production caller hands it a handle-band id (see
/// [`rule1_clear_object_descriptors_on_a_handle_id_is_a_no_op`]), which has no
/// shape — but nothing in the signature says so, and the function is reachable
/// from anywhere in the crate.
#[test]
fn rule1_clear_object_descriptors_transitions() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = shaped_object(&["alpha", "beta"]);
        let addr = obj as usize;
        set_property_attrs(addr, "alpha".to_string(), FROZEN_ATTRS);
        set_accessor_descriptor(addr, "beta".to_string(), accessor());
        let before = shapes::object_shape_stamp(obj);
        clear_object_descriptors(addr);
        assert_ne!(
            before,
            shapes::object_shape_stamp(obj),
            "clear_object_descriptors: a bulk clear is a descriptor change"
        );
        assert!(
            super::descriptor_state::get_accessor_descriptor(addr, "beta").is_none(),
            "clear_object_descriptors must actually drop the entries it transitions for"
        );
        assert!(
            super::descriptor_state::get_property_attrs(addr, "alpha").is_none(),
            "clear_object_descriptors must actually drop the entries it transitions for"
        );
    }
}

/// The handle path this function exists for: a recycled perry-ffi handle id.
/// It is not a heap cell, so there is nothing to transition — and the clear
/// must still empty the tables.
#[test]
fn rule1_clear_object_descriptors_on_a_handle_id_is_a_no_op() {
    let _lock = crate::gc::global_side_table_test_lock();
    // A handle-band id: small integer, no GcHeader, never a shaped object.
    let handle: usize = 0x41;
    assert!(
        crate::value::addr_class::is_handle_band(handle),
        "test premise: the id used by handle_expando_clear is handle-band"
    );
    set_property_attrs(handle, "hid".to_string(), FROZEN_ATTRS);
    assert!(super::descriptor_state::get_property_attrs(handle, "hid").is_some());
    clear_object_descriptors(handle);
    assert!(
        super::descriptor_state::get_property_attrs(handle, "hid").is_none(),
        "a recycled handle id must not inherit the previous tenant's descriptors"
    );
}

/// `transfer_descriptor_owner` is NOT a descriptor change: it re-keys one
/// logical object's entries after `js_array_grow` replaced its allocation.
/// The identity, the descriptor set and (for a shaped receiver) the ShapeId
/// all stay the same, so making no shape call is correct — but only while the
/// caller really has copied the header. Assert that contract rather than
/// documenting it.
#[test]
fn rule1_transfer_descriptor_owner_preserves_the_shape() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let old = shaped_object(&["alpha"]);
        let new = shaped_object(&["alpha"]);
        set_property_attrs(old as usize, "alpha".to_string(), FROZEN_ATTRS);
        // The two receivers took the same keyed transition (#10287), so they
        // share a ShapeId — which is exactly what an address change must not
        // disturb.
        let old_shape = shapes::object_shape_stamp(old);
        set_property_attrs(new as usize, "alpha".to_string(), FROZEN_ATTRS);
        assert_eq!(
            old_shape,
            shapes::object_shape_stamp(new),
            "test premise: an identical keyed descriptor install is shared"
        );
        transfer_descriptor_owner(old as usize, new as usize);
        assert_eq!(
            old_shape,
            shapes::object_shape_stamp(new),
            "transfer is a re-key of the same logical object: the ShapeId is a \
             fact about the object, not about its address"
        );
        assert!(
            super::descriptor_state::get_property_attrs(new as usize, "alpha").is_some(),
            "the moved entry must be readable through the new address"
        );
    }
}

/// The funnel (`note_descriptor_target_keyed`) sets
/// `OBJ_FLAG_HAS_DESCRIPTORS` and transitions the shape for
/// `GC_TYPE_OBJECT` ONLY. A typed array returns early before either; an array
/// or closure fails the `obj_type` test inside.
///
/// This is load-bearing for the emitted read path in two opposite directions:
///
/// * the descriptor FLAG is worthless as a guard for these receivers (it is
///   never set), so dropping the flag test loses nothing for them;
/// * the SHAPE is equally worthless (never transitioned), so a shape-only
///   guard must keep rejecting them by KIND. The GC-kind load cannot be
///   removed from the read path for non-`GC_TYPE_OBJECT` receivers until
///   their descriptor state is shape-carried too.
#[test]
fn rule1_funnel_does_not_cover_non_object_receivers() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let arr = crate::array::js_array_alloc(4);
        let addr = arr as usize;
        let header = crate::value::addr_class::try_read_gc_header(addr)
            .expect("a fresh array carries a GcHeader");
        assert_eq!(
            header.obj_type,
            crate::gc::GC_TYPE_ARRAY,
            "test premise: js_array_alloc produces GC_TYPE_ARRAY"
        );
        set_property_attrs(addr, "0".to_string(), FROZEN_ATTRS);
        assert!(
            !object_has_descriptors(addr),
            "the funnel only flags GC_TYPE_OBJECT — if this ever starts passing, \
             the read path's flag test became meaningful for arrays and this \
             test's conclusion below must be re-derived"
        );
        assert!(
            super::descriptor_state::get_property_attrs(addr, "0").is_some(),
            "the descriptor is still installed and observable: it is only the \
             shape-and-flag record of it that is missing"
        );
    }
}

/// Rule 2, the half the design doc left as "TO CONFIRM": a class id must
/// never fall inside the ShapeId range.
///
/// It matters because the read path tells a stamped object from an unstamped
/// one by RANGE alone — [`shapes::object_shape_stamp`] returns
/// `parent_class_id` when `is_shape_id` accepts it, and 0 otherwise. A class
/// id inside the range makes an unstamped object claim a live, unrelated
/// shape: `shape_record_by_id` then answers with someone else's ordered keys
/// and live-inline-slot bound.
#[test]
fn rule2_class_id_producers_stay_out_of_the_shape_id_range() {
    use super::class_registry::{SYNTHETIC_CLASS_ID_BASE, SYNTHETIC_CLASS_ID_END};
    // Compile-time: the two ranges are disjoint by construction.
    const _: () = assert!(
        SYNTHETIC_CLASS_ID_BASE >= shapes::SHAPE_ID_END
            || SYNTHETIC_CLASS_ID_END <= shapes::SHAPE_ID_BASE,
        "synthetic class ids and ShapeIds must occupy disjoint u32 ranges"
    );
    assert!(!shapes::is_shape_id(SYNTHETIC_CLASS_ID_BASE));
    assert!(!shapes::is_shape_id(SYNTHETIC_CLASS_ID_END - 1));
    // Every other class-id producer: codegen ids start at 1 and grow per
    // module; the builtin bands are 0x7FFF_FF00..=0x7FFF_FFFF and 0xFFFF_0000+.
    for id in [
        0u32,
        1,
        0x0077_8655,
        shapes::SHAPE_ID_BASE - 1,
        0x7FFF_FF30,
        0xFFFF_0000,
        0xFFFF_00D4,
        crate::object::NATIVE_MODULE_CLASS_ID,
    ] {
        assert!(
            !shapes::is_shape_id(id),
            "class id {id:#x} would be read back as a ShapeId"
        );
    }
}

/// The live allocator, not just the constants: minting a synthetic class id
/// must never produce a value an object's `+4` word would read back as a
/// shape. Sabotage-resistant because it mints from the real counter.
#[test]
fn rule2_minted_synthetic_class_ids_are_never_shape_ids() {
    let _lock = crate::gc::global_side_table_test_lock();
    let mut minted = Vec::new();
    for _ in 0..4 {
        let id = super::class_registry::test_alloc_synthetic_class_id();
        assert!(
            !shapes::is_shape_id(id),
            "a freshly minted synthetic class id {id:#x} lands in the ShapeId \
             range: an instance born with it as parent_class_id would alias a \
             live shape"
        );
        minted.push(id);
    }
    minted.dedup();
    assert_eq!(minted.len(), 4, "synthetic class ids must be unique");
}

/// Condition (ii) of the #10287 soundness argument, restated without the
/// descriptor flag: an unstamped object can never match a cached shape token.
///
/// It is a corollary of rule 2 and nothing else — which is why rule 2 is a
/// prerequisite for dropping the flag guard, not an independent nicety.
///
/// The `+4` word is written with a raw class id by five allocators
/// (`object/alloc.rs:190, 257, 315, 546, 720`) before any stamp lands on it,
/// and `js_object_alloc_class_dynamic_parent` then reads the PREDECESSOR
/// shape back out of that same word (`set_object_keys_array_with_live` ->
/// `object_shape_descriptor`). So the state exercised here is not
/// hypothetical: it is the window every dynamically-parented instance passes
/// through.
#[test]
fn rule2_unstamped_object_never_matches_a_cached_shape() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = js_object_alloc(0, 0);
        // Put the object back into the pre-stamp state the allocators leave
        // it in, with a real parent class id in the word.
        const PARENT: u32 = 0x0051_7A1D;
        (*obj).parent_class_id = PARENT;
        assert_eq!(
            shapes::object_shape_stamp(obj),
            0,
            "an unstamped object must report NO shape, so no cached token can match"
        );
        assert!(
            shapes::object_shape_descriptor(obj).is_none(),
            "and must resolve to no shape descriptor at all"
        );
    }
}

/// The producer end of the same invariant, on the live path: a synthetic
/// class id registered as a PARENT is what `js_object_alloc_class_dynamic_parent`
/// writes into `+4` of every instance of a `class X extends someFunction`.
/// While synthetic ids were minted from `0x8000_0000` — the ShapeId floor —
/// that word aliased a live shape, and the alias was not rare: both counters
/// started at the same value, so synthetic id #1 was shape #1.
#[test]
fn rule2_a_synthetic_parent_class_id_is_not_a_shape_token() {
    let _lock = crate::gc::global_side_table_test_lock();
    const CHILD: u32 = 0x0051_7A20;
    let synthetic = super::class_registry::test_alloc_synthetic_class_id();
    super::class_registry::register_class(CHILD, synthetic);
    let parent = super::class_meta_registry::get_parent_class_id(CHILD)
        .expect("the registry must hold the edge just registered");
    assert_eq!(parent, synthetic);
    assert!(
        !shapes::is_shape_id(parent),
        "the parent class id {parent:#x} that the dynamic-parent allocator \
         writes to +4 would be read back as a live ShapeId"
    );
    assert!(
        shapes::shape_record_by_id(parent).is_none(),
        "and it must resolve to no shape record"
    );
}
