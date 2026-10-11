//! Completed literal births use their seeded shape, not an Any birth followed
//! by five checked stores and another promotion. Only values belong to this
//! evaluation; key, representation and body admission facts belong to the shape.
use super::shapes_store;
use crate::object::{field_rep, shapes, static_shapes, ObjectHeader};

/// A complete per-use proof. Static ids are never reused, and their record
/// supplies keys and body identities; packed names/ABI entries are seed inputs.
fn birth_record(
    id: u32,
    count: u32,
    live: u32,
    rep: u64,
) -> Option<*mut shapes_store::ShapeRecord> {
    let record = shapes_store::ShapeSlab::agent_record_present(id)?;
    let r = unsafe { &*record };
    (r.object_kind() == shapes::ShapeObjectKind::Ordinary
        && r.proto_id == shapes::PROTO_ID_DEFAULT
        && r.logical_key_count == count
        && r.live_inline_slot_count == live
        && live >= count
        && count != 0
        && r.keys != 0
        && r.semantic_generation == 0
        && r.hole_count == 0
        && r.summary() == 0
        && r.integrity_flags() == 0
        && r.brands().is_empty()
        && field_rep::identity_with_special(r.rep) == rep
        && r.special_constfn_mask() != 0
        && r.constfn_birth_admitted())
    .then_some(record)
}

unsafe fn values_match(record: *const shapes_store::ShapeRecord, values: &[u64]) -> bool {
    let r = &*record;
    for entry in r.constfn_infos() {
        if crate::object::field_rep_store::constfn_store_info(values[entry.slot as usize])
            != Some(entry.info)
        {
            return false;
        }
    }
    let mut numeric = field_rep::f64_lane_slots(field_rep::identity_with_special(r.rep));
    while numeric != 0 {
        let slot = numeric.trailing_zeros();
        numeric &= numeric - 1;
        if field_rep::f64_slot_bits(values[slot as usize]).is_none() {
            return false;
        }
    }
    true
}

/// Compiler-owned complete literal facts. Inputs have been evaluated in source
/// order and rooted by generated code. The no-collect arm consumes their raw
/// words now; the collecting arm copies mutable roots before any allocation.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn js_object_alloc_constfn_static(
    requested: u32,
    packed: *const u8,
    packed_len: u32,
    count: u32,
    live: u32,
    class_id: u32,
    rep: u64,
    entries: *const static_shapes::ConstFnStaticEntry,
    entry_count: u32,
    values: *const u64,
) -> u64 {
    assert!(!values.is_null() && count > 0 && count <= field_rep::REP_SLOTS);
    let words = unsafe { std::slice::from_raw_parts(values, count as usize) };
    if let Some(record) = birth_record(requested, count, live, rep) {
        if unsafe { values_match(record, words) } {
            let width = (live as usize).max(crate::object::INLINE_SLOT_FLOOR);
            let size = std::mem::size_of::<ObjectHeader>() + width * 8;
            let raw = crate::arena::arena_alloc_gc_no_collect(size, 8, crate::gc::GC_TYPE_OBJECT)
                as *mut ObjectHeader;
            if !raw.is_null() {
                unsafe {
                    crate::object::alloc_basic::init_unpublished(raw, class_id, 0, 0);
                    fill(raw, requested, live, rep, words);
                }
                return raw as usize as u64;
            }
        }
    }
    collecting_birth(
        requested,
        packed,
        packed_len,
        count,
        live,
        class_id,
        rep,
        entries,
        entry_count,
        words,
    )
}

#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn collecting_birth(
    requested: u32,
    packed: *const u8,
    packed_len: u32,
    count: u32,
    live: u32,
    class_id: u32,
    rep: u64,
    entries: *const static_shapes::ConstFnStaticEntry,
    entry_count: u32,
    values: &[u64],
) -> u64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let roots: Vec<_> = values.iter().map(|&v| scope.root_nanbox_u64(v)).collect();
    let object = scope.root_raw_mut_ptr(crate::object::alloc_basic::object_alloc_unpublished(
        class_id, live,
    ));
    // Resolve after allocation. A worker's by-facts seed can already own an
    // equivalent id; missing requests still enter the same seed/interner.
    let id = if birth_record(requested, count, live, rep).is_some() {
        requested
    } else {
        static_shapes::js_shape_seed_plain_constfn(
            requested,
            packed,
            packed_len,
            count,
            live,
            rep,
            entries,
            entry_count,
        )
    };
    let record = birth_record(id, count, live, rep)
        .expect("compiler literal seed must describe its complete birth");
    let words: Vec<_> = roots.iter().map(|v| v.get_nanbox_u64()).collect();
    let (id, rep) = if unsafe { values_match(record, &words) } {
        (id, rep)
    } else {
        unsafe { compatible_shape(record, &words) }
    };
    // The compatible-facts mint may collect. Re-read both the newborn and
    // every value through their roots; no record or raw closure spans it.
    let words: Vec<_> = roots.iter().map(|v| v.get_nanbox_u64()).collect();
    let object = object.get_raw_mut_ptr::<ObjectHeader>();
    unsafe { fill(object, id, live, rep, &words) };
    object as usize as u64
}

/// A refused value changes the facts, not the construction algorithm. Resolve
/// its compatible representation through the sole interner before publication.
#[cold]
unsafe fn compatible_shape(record: *const shapes_store::ShapeRecord, values: &[u64]) -> (u32, u64) {
    let r = &*record;
    let mut rep = field_rep::identity_with_special(r.rep);
    let mut infos = Vec::new();
    for entry in r.constfn_infos() {
        if crate::object::field_rep_store::constfn_store_info(values[entry.slot as usize])
            == Some(entry.info)
        {
            infos.push(*entry);
        } else {
            rep = field_rep::with_slot_rep(rep, u32::from(entry.slot), field_rep::REP_ANY);
        }
    }
    let mut numeric = field_rep::f64_lane_slots(rep);
    while numeric != 0 {
        let slot = numeric.trailing_zeros();
        numeric &= numeric - 1;
        if field_rep::f64_slot_bits(values[slot as usize]).is_none() {
            rep = field_rep::with_slot_rep(rep, slot, field_rep::REP_ANY);
        }
    }
    let id = shapes::publish_shape_result(shapes::shape_descriptor_intern_with_special(
        r.keys as usize as *const crate::ArrayHeader,
        r.logical_key_count,
        r.live_inline_slot_count,
        0,
        shapes::ShapeObjectKind::Ordinary,
        0,
        shapes::PROTO_ID_DEFAULT,
        0,
        rep,
        &infos,
        &[],
        None,
        0,
    ));
    (id, rep)
}

/// No allocation or JS between slot fill and publication. A young birth owes
/// no old carrier note; an old birth keeps the existing note and bulk barrier.
unsafe fn fill(obj: *mut ObjectHeader, shape: u32, live: u32, rep: u64, values: &[u64]) {
    let fields = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
    for (slot, &bits) in values.iter().enumerate() {
        let bits = if field_rep::slot_rep(rep, slot as u32) == field_rep::REP_F64 {
            field_rep::f64_slot_bits(bits).expect("birth validated numeric lane")
        } else {
            bits
        };
        // GC_STORE_AUDIT(BARRIERED): complete fresh slots followed by the bulk newborn barrier.
        fields.add(slot).write(bits);
    }
    for slot in values.len()..(live as usize).max(crate::object::INLINE_SLOT_FLOOR) {
        // GC_STORE_AUDIT(INIT): physical slack remains undefined.
        fields.add(slot).write(crate::value::TAG_UNDEFINED);
    }
    shapes::store_kind::premark_plain_ordinary(obj);
    if crate::arena::pointer_in_nursery(obj as usize) {
        // GC_STORE_AUDIT(POINTER_FREE): the only publication is this final ShapeId.
        (*obj).parent_class_id = shape;
    } else {
        shapes::stamp_object_shape_id_with_carrier_note(obj, shape);
    }
    if crate::gc::newborn_parent_needs_barrier(obj as usize) {
        crate::gc::runtime_write_barrier_newborn_slots(obj as usize, fields, values.len());
    }
    shapes::store_kind::check_store_facts(obj);
}

#[cfg(test)]
#[path = "static_constfn_birth_tests.rs"]
mod tests;
