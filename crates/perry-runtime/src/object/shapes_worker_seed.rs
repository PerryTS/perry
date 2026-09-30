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

#[derive(Clone, Copy)]
struct SeedRecord {
    id: u32,
    keys: u64,
    logical_key_count: u32,
    live_inline_slot_count: u32,
    proto_id: u64,
    rep: u64,
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
        {
            seed.push(SeedRecord {
                id,
                keys: r.keys,
                logical_key_count: r.logical_key_count,
                live_inline_slot_count: r.live_inline_slot_count,
                proto_id: r.proto_id,
                rep: r.rep,
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
        let _ = super::shapes_slot_list::install_external_shape_id(
            r.id,
            r.keys as usize as *const crate::array::ArrayHeader,
            r.logical_key_count,
            r.live_inline_slot_count,
            r.proto_id,
            r.rep,
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
            let k = crate::object::js_build_class_keys_array(CID, 3, b"x\0y\0z\0".as_ptr(), 6)
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
}
