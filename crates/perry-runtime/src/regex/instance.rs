//! Ordinary RegExp births. The memo holds only a validated ShapeId; the
//! shape carries the private matcher, lastIndex attributes and prototype.
use super::{RegExpData, RegExpHeader};
use crate::codegen_abi::LayoutWord;
use crate::gc::RuntimeHandleScope;
use crate::value::js_nanbox_pointer;
use std::cell::Cell;

static PROTOTYPE_READ: crate::object::IntrinsicPrivateReadSite =
    crate::object::IntrinsicPrivateReadSite::new(
        &crate::object::runtime_layouts::intrinsic_private_atoms::REGEXP_PROTOTYPE,
    );
crate::perry_thread_local! {
    static BIRTH_SHAPE: Cell<u32> = const { Cell::new(0) };
}

/// A fresh RegExp around immutable matcher data. `data` reads the data cell's
/// current address from wherever the caller holds it (a root, the literal
/// site's registered word); it is read after the allocation, the only
/// collecting action here, so no handle is needed for it.
pub(super) fn new(data: impl FnOnce() -> *const RegExpData) -> *mut RegExpHeader {
    let shape = BIRTH_SHAPE.with(Cell::get);
    if shape != 0 {
        let size = std::mem::size_of::<RegExpHeader>() + crate::object::INLINE_SLOT_FLOOR * 8;
        let receiver = crate::arena::arena_alloc_gc_no_collect(size, 8, crate::gc::GC_TYPE_OBJECT)
            as *mut RegExpHeader;
        if !receiver.is_null() {
            // The birth record is an agent-lifetime external carrier. Its
            // two Any lanes cannot deprecate (only F64/ConstFn lanes do), so
            // no record lookup or layout validation belongs to this birth.
            unsafe {
                (*receiver).class_id = 0;
                (*receiver).parent_class_id = shape;
                // GC_STORE_AUDIT(INIT): unpublished nursery object's empty metadata.
                (*receiver).meta = std::ptr::null_mut();
                crate::object::shapes::store_kind::premark_plain_ordinary(receiver);
                let slots = receiver
                    .cast::<u8>()
                    .add(std::mem::size_of::<RegExpHeader>())
                    .cast::<u64>();
                let matcher = js_nanbox_pointer(data() as i64).to_bits();
                // GC_STORE_AUDIT(INIT): young receiver; shade the matcher below if marking.
                slots.write(matcher);
                // GC_STORE_AUDIT(INIT): lastIndex starts at Number zero on every evaluation.
                slots.add(1).write(0.0f64.to_bits());
                // The floor is exactly the two header words today, so the range is empty;
                // it fills the slack if the floor grows.
                #[allow(clippy::reversed_empty_ranges)]
                for i in 2..crate::object::INLINE_SLOT_FLOOR {
                    // GC_STORE_AUDIT(INIT): pointer-free physical slack for ordinary key adds.
                    slots.add(i).write(crate::value::TAG_UNDEFINED);
                }
                if !crate::gc::incremental_mark_barrier_globally_idle() {
                    crate::gc::runtime_shade_external_edge(matcher);
                }
            }
            return receiver;
        }
    }
    new_slow(LayoutWord(shape), data)
}

#[cold]
#[inline(never)]
fn new_slow(shape: LayoutWord, data: impl FnOnce() -> *const RegExpData) -> *mut RegExpHeader {
    let (mut receiver, born_on_memo) = crate::object::object_alloc_plain_born(2, shape);
    let rep = born_on_memo.unwrap_or_else(|| {
        if unsafe { crate::object::shapes::object_shape_stamp(receiver) } != shape {
            receiver = prepare_shape(receiver);
        }
        crate::object::shapes::shape_rep_of_receiver(unsafe {
            (crate::object::shapes::object_shape_stamp(receiver)).0
        })
    });
    let data = js_nanbox_pointer(data() as i64).to_bits();
    unsafe {
        let plain = crate::object::field_rep::slot_rep(rep, 0) == crate::object::field_rep::REP_ANY
            && crate::object::field_rep::slot_rep(rep, 1) == crate::object::field_rep::REP_ANY;
        if plain && !crate::gc::newborn_parent_needs_barrier(receiver as usize) {
            // The gate the emitted constructor stores use: a parent born in
            // the nursery owes no remembered-set entry, and no incremental
            // cycle is live to shade the child. Both lanes are `Any`, so the
            // bits are the values' own.
            let slots = (receiver as *mut u8).add(std::mem::size_of::<RegExpHeader>()) as *mut u64;
            // GC_STORE_AUDIT(INIT): newborn nursery receiver, globally idle barrier.
            slots.write(data);
            // GC_STORE_AUDIT(INIT): newborn receiver's lastIndex, a Number.
            slots.add(1).write(0.0f64.to_bits());
        } else {
            crate::object::store_object_field_slot_layout_deferred(receiver, 0, data);
            crate::object::store_object_field_slot_layout_deferred(receiver, 1, 0.0f64.to_bits());
        }
    }
    receiver
}

#[cold]
#[inline(never)]
fn prepare_shape(receiver: *mut RegExpHeader) -> *mut RegExpHeader {
    let scope = &RuntimeHandleScope::new();
    let receiver = &scope.root_raw_mut_ptr(receiver);
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
    let private_key = super::MATCHER_READ.birth_key();
    let proof = SharedLayout::shape_cache_entry();
    let keys = unsafe {
        crate::object::canonical_keys::extend_key_with_entry(
            &proof,
            CanonicalKeys::EMPTY,
            private_key,
            PRIVATE_FIELD_ENTRY,
        )
    };
    let first = keys;
    let keys = scope.root_raw_mut_ptr(keys.as_ptr());
    let index_key = crate::string::intern_ascii_literal(b"lastIndex");
    let final_keys = keys.with_mut_ptr::<crate::array::ArrayHeader, _>(|keys| unsafe {
        crate::object::canonical_keys::extend_key_with_entry(
            &proof,
            first.rooted_at(keys),
            index_key,
            attr_bits_to_entry(1),
        )
    });
    // No collecting operation between the final canonical keys and their
    // publication into the rooted receiver's shape.
    receiver.with_mut_ptr::<RegExpHeader, _>(|r| unsafe {
        assert!(crate::object::shapes::stamp_linked_final_shape(
            r,
            final_keys,
            2,
            proto_id,
            prototype.with_const_ptr::<RegExpHeader, _>(|p| js_nanbox_pointer(p as i64).to_bits()),
            |_| false,
        ));
        let shape = crate::object::shapes::object_shape_stamp(r);
        // `BIRTH_SHAPE` holds this id for the agent's life and re-stamps it
        // with no receiver in between, so the record is an external carrier,
        // as every agent-lifetime intrinsic birth memo's is (the base
        // Function shapes, `closure::shape::mint`): a full trace that finds
        // no live RegExp must not retire it and hand the next birth a new
        // ShapeId (#12313). Its keys and prototype words stay repaired by the
        // shape table's scanner while no receiver carries it.
        crate::object::shapes::note_external_shape_carrier(
            crate::object::shapes::shape_descriptor_for_word(shape),
        );
        assert_eq!(
            crate::object::shapes::shape_rep_for_word(shape),
            0,
            "the fixed RegExp birth must have only Any lanes"
        );
        BIRTH_SHAPE.with(|memo| memo.set(shape.0));
    });
    receiver.with_mut_ptr::<RegExpHeader, _>(|r| r)
}

/// The builtin-RegExp fact: `value` is a RegExp still on this agent's birth
/// layout ([`birth_layout`]), and `%RegExp.prototype%` has not been written
/// since the realm sealed it. Then every property the RegExp operations read
/// is the builtin: the birth layout's own keys are only `[[RegExpMatcher]]`
/// (slot 0) and a writable data `lastIndex` (slot 1), its `[[Prototype]]` is
/// the intrinsic, and `exec`, `test`, the flag getters and the `@@replace`/
/// `@@match`/`@@split`/`@@search`/`@@matchAll` methods are that prototype's
/// own builtins. A prototype write sets its shadow bit, which this reads.
///
/// Leaf, NoCollect, NoThrow. Returns the receiver for the slot accessors.
#[inline(always)]
pub(crate) fn builtin(value: f64) -> Option<*mut RegExpHeader> {
    let receiver = birth_layout(value)?;
    crate::object::builtin_proto::prototype_unwritten(
        crate::object::builtin_proto::BuiltinProto::RegExp,
    )
    .then_some(receiver)
}

/// The receiver half of [`builtin`]: `value` is a RegExp still on this
/// agent's birth layout. An own key, an attribute change, a `[[Prototype]]`
/// relink or a subclass birth gives the receiver another layout word, so this
/// proves the receiver's own keys and its `[[Prototype]]` only; what the
/// prototype currently holds is a separate fact (`builtin`'s shadow bit, or
/// `regex_read_sites::flag_getters`' proof of the flag accessors alone).
///
/// Leaf, NoCollect, NoThrow.
#[inline(always)]
pub(crate) fn birth_layout(value: f64) -> Option<*mut RegExpHeader> {
    let header = crate::value::addr_class::pointer_value_header(value.to_bits())?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    let shape = BIRTH_SHAPE.with(Cell::get);
    let receiver = (value.to_bits() & crate::value::POINTER_MASK) as *mut RegExpHeader;
    // The header's first word is `class_id` (0 for every RegExp) and the
    // layout word, whose shadow bit is clear on the birth layout.
    (shape != 0 && unsafe { std::ptr::read(receiver as *const u64) } == u64::from(shape) << 32)
        .then_some(receiver)
}

/// The birth layout's inline slot `index` of a receiver [`builtin`] admitted.
#[inline(always)]
unsafe fn slot(receiver: *mut RegExpHeader, index: usize) -> *mut u64 {
    receiver
        .cast::<u8>()
        .add(std::mem::size_of::<RegExpHeader>())
        .cast::<u64>()
        .add(index)
}

/// The matcher data of a receiver [`builtin`] admitted.
///
/// # Safety
/// `receiver` was admitted by [`builtin`] with no collection since.
#[inline(always)]
pub(crate) unsafe fn builtin_data(receiver: *mut RegExpHeader) -> *const RegExpData {
    crate::value::js_nanbox_get_pointer(f64::from_bits(*slot(receiver, 0))) as *const RegExpData
}

/// `lastIndex` of a receiver [`builtin`] admitted: its own data slot.
///
/// # Safety
/// As for [`builtin_data`].
#[inline(always)]
pub(crate) unsafe fn builtin_last_index(receiver: *mut RegExpHeader) -> f64 {
    f64::from_bits(*slot(receiver, 1))
}

/// `Set(R, "lastIndex", 0, true)` on a receiver [`builtin`] admitted: its
/// lastIndex is a writable data property in an `Any` slot, so the Set is this
/// store, and a Number needs no write barrier.
///
/// # Safety
/// As for [`builtin_data`].
#[inline(always)]
pub(crate) unsafe fn builtin_reset_last_index(receiver: *mut RegExpHeader) {
    // GC_STORE_AUDIT(POINTER_FREE): a Number into an Any lane; no barrier.
    *slot(receiver, 1) = 0.0f64.to_bits();
}

// Read the realm intrinsic through the same private slot machinery as matcher
// data. Neither this read nor BIRTH_SHAPE retains a managed pointer.
pub(crate) fn intrinsic_prototype() -> f64 {
    let global = crate::object::js_get_global_this();
    PROTOTYPE_READ
        .read(global)
        .expect("realm has installed the RegExp intrinsic")
}
