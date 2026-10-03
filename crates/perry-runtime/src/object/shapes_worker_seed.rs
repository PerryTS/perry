//! Charter step 5, P4: a worker agent sees every codegen ShapeId with its rep.
//!
//! A `perry/thread` worker never runs module initialization, so the ShapeIds
//! module init minted (class birth shapes, codegen literal shapes) are absent
//! from its fresh agent table. The inline bump allocator stamps such an id
//! with no runtime call, so the first allocation of a class in a worker can be
//! an inline one: the id must already name its descriptor, with its rep, in
//! that agent. The spawner therefore hands the worker its external carriers
//! (the ids codegen holds in module globals), and the worker installs them
//! before it runs any user code. This is a copy of the spawner's own facts,
//! taken at spawn; nothing process-global is kept.

use super::shapes_store::RECORD_FLAG_EXTERNAL_CARRIER;
use super::ShapeObjectKind;

#[derive(Clone)]
struct SeedRecord {
    id: u32,
    names: Vec<Vec<u8>>,
    logical_key_count: u32,
    live_inline_slot_count: u32,
    proto_id: u64,
    rep: u64,
    infos: Vec<super::shapes_store::ConstFnSlotInfo>,
    to_any: u32,
}

/// The spawner agent's external-carrier ShapeIds, captured on the spawning
/// thread and installed by [`install_worker_shape_seed`] on the worker.
#[derive(Clone)]
pub(crate) struct WorkerShapeSeed(std::sync::Arc<[SeedRecord]>);

/// Capture the calling agent's external carriers: ordinary, hole-free
/// records, exactly the facts `install_external_shape_id` rebuilds.
pub(crate) fn worker_shape_seed() -> WorkerShapeSeed {
    let table = &crate::state::state().shapes;
    let mut seed = Vec::new();
    table.slab().for_each(|id, record| {
        // SAFETY: live slab record, single-threaded agent, read immediately.
        let r = unsafe { *record };
        if r.has(RECORD_FLAG_EXTERNAL_CARRIER)
            && r.hole_count == 0
            && r.object_kind() == ShapeObjectKind::Ordinary
            && r.semantic_generation == 0
            && r.summary() == 0
            // A record whose identity names a prototype object holds a
            // pointer into THIS agent's heap (`shapes_prototype`); a worker
            // mints its own.
            && !super::proto_id_carries_word(r.proto_id)
        {
            // Copy key BYTES while the source agent owns the record. A worker
            // builds its own canonical keys, so no moving source key pointer
            // is held in the Rust-owned seed snapshot.
            let mut names = Vec::new();
            let (slots, len) = unsafe {
                crate::object::keys_array_dense_slots(
                    r.keys as usize as *const crate::array::ArrayHeader,
                )
            };
            if slots.is_null() || len < r.logical_key_count as usize {
                return;
            }
            for slot in 0..r.logical_key_count as usize {
                let mut short = [0; crate::value::SHORT_STRING_MAX_LEN];
                let Some(bytes) = (unsafe {
                    crate::string::js_string_key_bytes(
                        crate::JSValue::from_bits((*slots.add(slot)).to_bits()),
                        &mut short,
                    )
                }) else {
                    return;
                };
                names.push(bytes.to_vec());
            }
            seed.push(SeedRecord {
                id,
                names,
                logical_key_count: r.logical_key_count,
                live_inline_slot_count: r.live_inline_slot_count,
                proto_id: r.proto_id,
                rep: r.rep,
                infos: r.constfn_infos().to_vec(),
                to_any: r.deprecation_targets().1,
            });
        }
    });
    WorkerShapeSeed(seed.into())
}

/// Install `seed` in the calling (worker) agent. Runs before any user code of
/// the worker, so every id is absent and the install cannot disagree; an
/// install that still fails leaves the id absent, which only costs the
/// outlined allocator its exact-descriptor fallback.
pub(crate) fn install_worker_shape_seed(seed: &WorkerShapeSeed) {
    for r in seed.0.iter() {
        let names: Vec<&[u8]> = r.names.iter().map(Vec::as_slice).collect();
        let keys = unsafe { crate::object::static_shapes::canonical_keys_for_names(&names) };
        let _ = super::shapes_slot_list::install_external_shape_id_with_constfn(
            r.id,
            keys.arr(),
            r.logical_key_count,
            r.live_inline_slot_count,
            r.proto_id,
            ShapeObjectKind::Ordinary,
            r.rep,
            &r.infos,
            r.to_any,
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::object::field_rep::REP_F64;
    use crate::object::shapes::{js_object_shape_id_for_class_keys, shape_descriptor_by_id};

    const CID: u32 = 0x5118;

    /// A `perry/thread` worker never runs module init, and the inline bump
    /// allocator stamps a module-init ShapeId with no runtime call. So the
    /// worker's FIRST allocation of a class can be an inline one: the id must
    /// already name its descriptor, with its compiled rep, in the worker's
    /// agent before any outlined allocation installs it there.
    #[test]
    fn a_worker_sees_a_module_init_class_id_with_its_rep_before_any_allocation() {
        let rep = REP_F64 | (REP_F64 << 4);
        let (id, seed) = std::thread::spawn(move || {
            crate::object::class_image::enter_current_thread_image();
            let k = crate::object::js_build_class_keys_array(CID, 3, b"x\0y\0z\0".as_ptr(), 6, rep)
                as usize as u64;
            let id = js_object_shape_id_for_class_keys(k, 3, CID, rep);
            let image = crate::object::class_image::current_image_handle();
            let seed = super::worker_shape_seed();
            let worker = std::thread::spawn(move || {
                crate::object::class_image::adopt_image(image);
                let _agent = crate::agent::enter_worker_agent();
                crate::gc::ensure_gc_initialized();
                let before = shape_descriptor_by_id(id).is_some();
                super::install_worker_shape_seed(&seed);
                (
                    before,
                    shape_descriptor_by_id(id).map(|d| (d.rep, d.logical_key_count)),
                )
            })
            .join()
            .expect("worker panicked");
            (id, worker)
        })
        .join()
        .expect("spawner panicked");
        let (before, after) = seed;
        assert!(
            !before,
            "a fresh worker agent must not already hold id {id}"
        );
        assert_eq!(
            after,
            Some((rep, 3)),
            "the worker must hold id {id} with its compiled rep"
        );
    }

    /// The outlined compiled allocator receives the spawning image's keys
    /// global even though the worker rebuilt the same ShapeId with its own
    /// canonical array. It must allocate from the local descriptor, without
    /// evaluating the producer module or reading the foreign array's header.
    #[test]
    fn outlined_worker_birth_uses_the_seeded_local_keys_instead_of_the_spawners_global() {
        const WORKER_CID: u32 = 0x5119;
        std::thread::spawn(|| {
            crate::object::class_image::enter_current_thread_image();
            let rep = REP_F64;
            let source_keys =
                crate::object::js_build_class_keys_array(WORKER_CID, 2, b"x\0m\0".as_ptr(), 4, rep)
                    as usize;
            let id = js_object_shape_id_for_class_keys(source_keys as u64, 2, WORKER_CID, rep);
            let image = crate::object::class_image::current_image_handle();
            let seed = super::worker_shape_seed();
            std::thread::spawn(move || {
                crate::object::class_image::adopt_image(image);
                let worker_agent = crate::agent::enter_worker_agent();
                crate::gc::ensure_gc_initialized();
                super::install_worker_shape_seed(&seed);
                let descriptor = shape_descriptor_by_id(id).expect("seeded birth shape");
                assert_ne!(descriptor.keys as usize, source_keys);
                assert!(
                    unsafe { crate::value::addr_class::try_read_tracked_gc_header(source_keys) }
                        .is_none(),
                    "the source array must belong to another arena"
                );
                let handles = crate::gc::RuntimeHandleScope::new();
                let key = crate::string::js_string_from_bytes(b"x".as_ptr(), 1);
                let key = handles.root_raw_mut_ptr(key);
                let object = crate::object::js_object_alloc_class_inline_keys_stamped(
                    WORKER_CID,
                    0,
                    2,
                    source_keys as *mut crate::array::ArrayHeader,
                    id,
                    rep,
                );
                let object = handles.root_raw_mut_ptr(object);
                assert_eq!(
                    unsafe {
                        object.with_mut_ptr(|object_ptr| {
                            crate::object::shapes::object_shape_stamp(object_ptr)
                        })
                    },
                    id,
                    "the first outlined birth must retain the seeded id"
                );
                object.with_mut_ptr(|object_ptr| {
                    key.with_const_ptr(|key_ptr| {
                        crate::object::js_object_set_field_by_name(object_ptr, key_ptr, 4.0)
                    })
                });
                assert_eq!(
                    object
                        .with_const_ptr(|object_ptr| key.with_const_ptr(|key_ptr| {
                            crate::object::js_object_get_field_by_name(object_ptr, key_ptr)
                        }))
                        .as_number(),
                    4.0
                );
                assert_eq!(shape_descriptor_by_id(id).expect("birth shape").rep, rep);
                drop(handles);
                crate::agent::retire_agent(worker_agent);
            })
            .join()
            .expect("worker panicked");
        })
        .join()
        .expect("spawner panicked");
    }
}
