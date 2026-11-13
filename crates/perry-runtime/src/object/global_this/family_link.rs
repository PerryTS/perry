//! External per-agent words of a family. Both materialize arms call here;
//! every managed cell and every member already exists at this point.
use super::*;

#[inline]
fn bits(addr: usize) -> u64 {
    crate::value::js_nanbox_pointer(addr as i64).to_bits()
}

/// First-realm identity words are never replaced by a later vm/eval realm.
pub(crate) fn link_named_prototype_first(serial: u64, proto: usize) {
    if proto != 0 && crate::object::shapes::identity_prototype_word(serial) == 0 {
        crate::object::proto_validity::link_intrinsic_prototype_mark();
        crate::object::shapes::write_identity_word(serial, bits(proto));
    }
}

/// Link a builtin's completed cells. No property installation, prototype
/// ensure, managed allocation, or collection is permitted in this body.
#[cold]
#[inline(never)]
#[optimize(size)]
pub(crate) unsafe fn builtin(
    declaration: &BuiltinConstructorDeclaration,
    ctor: usize,
    proto: usize,
    _global: *mut ObjectHeader,
) {
    use std::sync::atomic::Ordering;
    if declaration.identity == crate::object::BuiltinConstructorIdentity::Error && ctor != 0 {
        ERROR_CONSTRUCTOR_PTR.with(|cell| cell.set(ctor));
    }
    if declaration.identity == crate::object::BuiltinConstructorIdentity::Function && proto != 0 {
        crate::closure::shape::FUNCTION_PROTOTYPE_PTR.store(proto as i64, Ordering::Release);
    }
    if declaration.identity == crate::object::BuiltinConstructorIdentity::Object && proto != 0 {
        crate::object::OBJECT_INTRINSIC_PTR.store(ctor as i64, Ordering::Release);
        crate::object::OBJECT_INTRINSIC_PROTO_PTR.store(proto as i64, Ordering::Release);
        crate::array::note_object_prototype_intrinsic(proto);
    }
    if proto != 0 {
        if let Some(family) = crate::object::builtin_proto::constructor_family(declaration) {
            let holder = if declaration.identity == crate::object::BuiltinConstructorIdentity::Array
            {
                crate::array::array_property_bag_resolved(proto as *const crate::array::ArrayHeader)
            } else {
                proto as *mut ObjectHeader
            };
            intrinsic_proto(family, holder);
        }
        super::populate::finish_builtin_prototype(declaration, proto as *mut ObjectHeader);
        if let Some(serial) = declaration.prototype_serial {
            link_named_prototype_first(serial, proto);
        }
        if let Some(serial) = declaration.named_serial {
            let holder = crate::array::array_property_bag_resolved(
                proto as *const crate::array::ArrayHeader,
            );
            link_named_prototype_first(serial, holder as usize);
        }
        if let Some(serial) = declaration.sealed_serial {
            crate::object::proto_validity::link_intrinsic_prototype_mark();
            crate::object::builtin_proto::link_runtime_prototype(
                proto as *mut ObjectHeader,
                serial,
            );
        }
        #[cfg(feature = "regex-engine")]
        if declaration.identity == crate::object::BuiltinConstructorIdentity::RegExp
            && !_global.is_null()
        {
            regexp_private(_global, proto);
        }
    }
    if declaration.identity.is_typed_array() && ctor != 0 {
        let parent = crate::closure::props::born_state_prototype(ctor)
            .expect("typed constructor lacks its declared dictionary state");
        let parent_ctor = (parent & crate::value::POINTER_MASK) as usize;
        let parent_proto =
            crate::array::family_intrinsic_cells(crate::codegen_abi::REALM_FAMILY_TYPED_ARRAY).0;
        typed_array(parent_ctor, parent_proto);
    }
    crate::array::note_realm_intrinsic(declaration.identity, proto, ctor);
}

/// GLOBAL_OBJECT declares exactly one private key, the RegExp intrinsic.
/// Read that slot ordinal from its actual row, including a reduced image row.
#[cfg(feature = "regex-engine")]
unsafe fn regexp_private(global: *mut ObjectHeader, prototype: usize) {
    let row = crate::object::shapes::object_shape_record(global)
        .expect("realm global has its published row");
    let keys = row.keys_view();
    let count = row.logical_key_count();
    let index = (0..count)
        .find(|&index| {
            crate::object::key_attrs::entry_is_private(crate::object::key_attrs::keys_entry(
                keys.arr(),
                index,
            ))
        })
        .expect("realm global lacks its declared private intrinsic slot");
    let value = bits(prototype);
    // The row selects the physical representation. BIND must not allocate a
    // missing slot or perform a program property mutation.
    let slot = match crate::object::key_attrs::list_storage_slot(keys.arr(), index) {
        Some(slot) => slot,
        None => {
            let table = keys.arr().cast::<crate::object::overflow::Table>();
            match crate::object::overflow::fixed_slot_of(table, index) {
                Some(slot) => slot,
                None => {
                    let address = crate::object::overflow::value_at(table, index);
                    // GC_STORE_AUDIT(BARRIERED): overwrite the existing traced
                    // overflow value selected by the global's actual row.
                    address.write(value);
                    crate::gc::runtime_write_barrier_slot(table as usize, address as usize, value);
                    return;
                }
            }
        }
    };
    if slot >= row.live_inline_slot_count() {
        let meta = (*global).meta;
        assert!(
            !meta.is_null(),
            "realm global lacks its prepared private spill"
        );
        let spill = (*meta).spill as *const crate::array::ArrayHeader;
        assert!(
            !spill.is_null() && slot < (*spill).length,
            "realm global lacks its prepared private spill slot"
        );
        let address = crate::array::array_elements_ptr(spill)
            .cast::<u64>()
            .add(slot as usize);
        assert_ne!(*address, crate::value::TAG_HOLE);
        // GC_STORE_AUDIT(BARRIERED): BIRTH prepared this traced pointer slot;
        // linking repeats its value without a program write or layout edit.
        address.write(value);
        crate::gc::runtime_write_barrier_slot(spill as usize, address as usize, value);
        return;
    }
    let address = global
        .cast::<u8>()
        .add(std::mem::size_of::<ObjectHeader>())
        .cast::<u64>()
        .add(slot as usize);
    // GC_STORE_AUDIT(BARRIERED): overwrite the existing traced inline slot.
    address.write(value);
    crate::gc::runtime_write_barrier_slot(global as usize, address as usize, value);
}

/// The two roots of the abstract TypedArray family.
pub(crate) fn typed_array(ctor: usize, proto: usize) {
    use std::sync::atomic::Ordering;
    crate::array::note_family_intrinsic(crate::codegen_abi::REALM_FAMILY_TYPED_ARRAY, proto, ctor);
    crate::object::TYPED_ARRAY_INTRINSIC_PTR.store(ctor as i64, Ordering::Release);
    crate::object::TYPED_ARRAY_INTRINSIC_PROTO_PTR.store(proto as i64, Ordering::Release);
}

/// A function tower's own serial is its root fact, for both main's cells
/// and the cells already emitted by the image.
pub(crate) fn function_tower(serial: u64, ctor: usize, proto: usize, generator: usize) {
    use perry_abi::intrinsic_serials::{
        INTRINSIC_SERIAL_ASYNC_FUNCTION, INTRINSIC_SERIAL_ASYNC_GENERATOR_FUNCTION,
        INTRINSIC_SERIAL_GENERATOR_FUNCTION,
    };
    use std::sync::atomic::Ordering;
    let (ctor_slot, proto_slot, generator_slot) = match serial {
        INTRINSIC_SERIAL_ASYNC_FUNCTION => (
            &crate::object::ASYNC_FUNCTION_INTRINSIC_PTR,
            &crate::object::ASYNC_FUNCTION_INTRINSIC_PROTO_PTR,
            None,
        ),
        INTRINSIC_SERIAL_GENERATOR_FUNCTION => (
            &crate::object::GENERATOR_FUNCTION_INTRINSIC_PTR,
            &crate::object::GENERATOR_INTRINSIC_PROTO_PTR,
            Some(&crate::object::GENERATOR_PROTOTYPE_PTR),
        ),
        INTRINSIC_SERIAL_ASYNC_GENERATOR_FUNCTION => (
            &crate::object::ASYNC_GENERATOR_FUNCTION_INTRINSIC_PTR,
            &crate::object::ASYNC_GENERATOR_INTRINSIC_PROTO_PTR,
            Some(&crate::object::ASYNC_GENERATOR_PROTOTYPE_PTR),
        ),
        _ => return,
    };
    crate::array::note_family_intrinsic(
        crate::codegen_abi::REALM_FAMILY_FUNCTION_TOWERS,
        proto,
        ctor,
    );
    ctor_slot.store(ctor as i64, Ordering::Release);
    if let Some(slot) = generator_slot {
        slot.store(generator as i64, Ordering::Release);
    }
    link_named_prototype_first(serial, proto);
    // Root last: an ensure never observes a partly published tower.
    proto_slot.store(proto as i64, Ordering::Release);
}

/// The iterator group publishes its existing eight slots from the same
/// ordered birth facts. The shared root is stored last.
pub(crate) fn iterators(cells: [usize; 8]) {
    crate::array::note_family_intrinsic(crate::codegen_abi::REALM_FAMILY_ITERATORS, cells[0], 0);
    use crate::object::iterator_prototypes as tower;
    use std::sync::atomic::Ordering;
    for (cell, slot) in cells
        .into_iter()
        .zip([
            &tower::ITERATOR_PROTOTYPE_PTR,
            &tower::ARRAY_ITERATOR_PROTOTYPE_PTR,
            &tower::MAP_ITERATOR_PROTOTYPE_PTR,
            &tower::SET_ITERATOR_PROTOTYPE_PTR,
            &tower::STRING_ITERATOR_PROTOTYPE_PTR,
            &tower::REGEXP_STRING_ITERATOR_PROTOTYPE_PTR,
            &tower::ITERATOR_HELPER_PROTOTYPE_PTR,
            &tower::WRAP_FOR_VALID_ITERATOR_PROTOTYPE_PTR,
        ])
        .skip(1)
    {
        slot.store(cell as i64, Ordering::Release);
    }
    for (index, cell) in cells.into_iter().enumerate() {
        link_named_prototype_first(
            perry_abi::intrinsic_serials::INTRINSIC_SERIAL_ITERATOR_PROTOTYPE + index as u64,
            cell,
        );
    }
    if cells[0] != 0 {
        crate::object::iterator_ctor::note_birth_keys(cells[0] as *mut ObjectHeader);
    }
    tower::ITERATOR_PROTOTYPE_PTR.store(cells[0] as i64, Ordering::Release);
}

/// Root publication of a builtin prototype, shared by independent Object
/// birth and the generic family materialize arms.
pub(crate) fn intrinsic_proto(
    family: crate::object::builtin_proto::BuiltinProto,
    prototype: *mut ObjectHeader,
) {
    if !prototype.is_null() {
        crate::object::proto_validity::link_intrinsic_prototype_mark();
        crate::agent_ptrs::publish_intrinsic_proto(family as usize, prototype);
    }
}
