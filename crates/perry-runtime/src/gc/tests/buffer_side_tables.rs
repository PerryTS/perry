//! Death pruning for the attribute tables keyed by a buffer's address.
//!
//! A buffer's brand is its GC type byte (#10694), so it dies with the cell;
//! what `finalize_collected_dead_buffer` still has to drop are the attributes
//! (own properties, view records, Uint8Array integrity state, CryptoKey
//! metadata, ...), and it now runs from the `BufferSideTables` finalize hook
//! of the sweep that frees the cell. Every "pruned" test below therefore also
//! proves the hook is wired: without it nothing calls the finalizer.
//!
//! A dead address is probed only through the ALLOCATOR-checked header read
//! (`live_buffer_type`): a swept block may have been reset or released, and
//! the plain recognizers' contract is a live cell.

use super::super::*;
use super::support::*;

fn full_gc() {
    let _ =
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
}

/// The buffer-family type of `addr` if the allocator still owns a live header
/// there — safe on an address whose cell (and maybe block) is gone.
fn live_buffer_type(addr: usize) -> Option<u8> {
    let header = unsafe { crate::value::addr_class::try_read_tracked_gc_header(addr) }?;
    let obj_type = unsafe { header.as_ref() }.obj_type;
    crate::gc::is_buffer_family_type(obj_type).then_some(obj_type)
}

/// A dead `DataView` / SAB-flagged buffer gives up its brand with its cell: the
/// sweep invalidates the header, so a recycled address cannot answer to
/// `util.types.isDataView` / `isSharedArrayBuffer` (the #6080 ABA class the old
/// registries needed explicit pruning for, #6337).
#[test]
fn test_dead_data_view_and_sab_brands_die_with_their_cells() {
    let _guard = GcTestIsolationGuard::new();

    let view = (crate::buffer::bytes::from_slice(crate::buffer::bytes::Brand::DataView, &[0; 32])
        .to_bits()
        & crate::value::POINTER_MASK) as usize;
    let sab = crate::buffer::store::alloc_test(crate::gc::GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER, 32)
        as usize;

    assert_eq!(
        live_buffer_type(view),
        Some(crate::gc::GC_TYPE_BUFFER_DATA_VIEW | crate::codegen_abi::BYTES_TYPE_VIEW)
    );
    assert_eq!(
        live_buffer_type(sab),
        Some(crate::gc::GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER)
    );

    // No roots: dead at the full trace. (Buffers are TENURED old-gen residents,
    // so only a FULL trace can prove them dead.)
    full_gc();

    assert_eq!(live_buffer_type(view), None);
    assert_eq!(live_buffer_type(sab), None);
}

/// The safety inverse: a LIVE (rooted) view keeps its brand. Full mark-sweep is
/// non-moving, so the rooted buffers keep their addresses.
///
/// `CopyingNurseryTestGuard::new(2)` — not `GcTestIsolationGuard` — because
/// only it pushes the shadow frame that makes `js_shadow_slot_set` an actual
/// root. Under the plain isolation guard the slot writes land in no frame, the
/// buffers stay unreachable, and the test would "pass" for the wrong reason.
#[test]
fn test_live_data_view_and_shared_array_buffer_flags_survive_full_gc() {
    let _guard = CopyingNurseryTestGuard::new(2);

    let view = (crate::buffer::bytes::from_slice(crate::buffer::bytes::Brand::DataView, &[0; 32])
        .to_bits()
        & crate::value::POINTER_MASK) as usize;
    let sab = crate::buffer::store::alloc_test(crate::gc::GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER, 32)
        as usize;

    js_shadow_slot_set(0, ptr_bits(view));
    js_shadow_slot_set(1, ptr_bits(sab));

    full_gc();

    assert!(
        crate::buffer::is_data_view(view),
        "a live (rooted) DataView must keep its registry entry"
    );
    assert!(
        crate::buffer::is_shared_array_buffer(sab),
        "a live (rooted) SAB-flagged buffer must keep its registry entry"
    );
}

/// A real `new SharedArrayBuffer(n)` backing is process-global: it survives a
/// full collection with no roots at all (no collector sweeps it), and stays
/// recognisable through every predicate — its header brand and the
/// process-global registry the cross-thread serializer and `Atomics` futex
/// keying depend on.
#[test]
fn test_process_global_sab_backing_survives_full_gc_unrooted() {
    let _guard = GcTestIsolationGuard::new();

    let buf = crate::buffer::js_shared_array_buffer_new(64);
    let addr = crate::shared_sab::shared_store_owner(buf as usize).unwrap();
    assert!(crate::shared_sab::is_shared_sab(addr));

    // Deliberately unrooted. The backing is never freed, so this must be a
    // no-op for it — a SAB is reachable from other agents, not from this
    // thread's roots.
    full_gc();

    assert!(
        crate::shared_sab::is_shared_sab(addr),
        "the process-global SAB registry must still recognise the backing"
    );
    assert!(
        crate::buffer::is_shared_array_buffer(addr),
        "the SAB must still answer util.types.isSharedArrayBuffer"
    );
    assert!(
        crate::buffer::is_registered_buffer(addr),
        "the SAB must still answer as a registered buffer"
    );
}

/// A dead buffer's OWN-PROPERTY entry must go too (#6406's table).
///
/// `finalize_collected_dead_buffer` prunes the address-keyed tables; this
/// one was once missing. Its only clear site was `register_buffer`, which fires
/// only when the recycled address is re-issued to another *buffer* — so an
/// entry whose address is never reused, or is reused by a plain object,
/// survived for the life of the process. That matters more here than for the
/// identity registries above, because `scan_buffer_own_props_roots_mut` traces
/// the stored VALUES in every GC phase: the dead buffer's expando closure, and
/// everything it captures, stayed reachable forever.
#[test]
fn test_dead_buffer_own_property_entry_pruned_on_full_gc() {
    let _guard = GcTestIsolationGuard::new();

    let addr = crate::buffer::buffer_alloc(32) as usize;
    crate::buffer::buffer_set_own_prop(addr, "tag", 7.0);
    assert_eq!(
        crate::buffer::buffer_get_own_prop(addr, "tag"),
        Some(7.0),
        "test premise: the own property is recorded"
    );

    // No roots: dead at the full trace. (Buffers are TENURED old-gen residents,
    // so only a FULL trace can prove them dead.)
    full_gc();

    assert!(
        live_buffer_type(addr).is_none(),
        "the owner must really die"
    );
}

/// Buffer-backed Uint8Arrays use TypedArray metadata tables even though their
/// owners are swept through the buffer finalizer. The finalizer must clear the
/// non-extensible marker before the old-arena address can be reused (#9347).
#[test]
fn test_dead_uint8array_extensibility_entry_pruned_on_full_gc() {
    let _guard = GcTestIsolationGuard::new();

    let addr = crate::buffer::js_uint8array_alloc(32) as usize;
    assert!(unsafe {
        crate::typedarray_props::typed_array_set_property_by_name(addr, "existing", 1.0)
    });
    assert_eq!(
        crate::buffer::buffer_get_own_prop(addr, "existing"),
        Some(1.0),
        "the typed-array Set path must use the canonical Buffer property table"
    );
    crate::typedarray_props::typed_array_mark_no_extend(addr);
    assert!(
        crate::typedarray_props::typed_array_owner_no_extend(addr),
        "test premise: the Uint8Array has a side-table marker"
    );
    assert!(unsafe {
        crate::typedarray_props::typed_array_set_property_by_name(addr, "existing", 2.0)
    });
    assert_eq!(
        crate::buffer::buffer_get_own_prop(addr, "existing"),
        Some(2.0)
    );
    assert!(!unsafe {
        crate::typedarray_props::typed_array_set_property_by_name(addr, "fresh", 3.0)
    });
    crate::buffer::buffer_set_own_prop(addr, "direct_fresh", 4.0);
    assert!(
        crate::buffer::buffer_get_own_prop(addr, "fresh").is_none()
            && crate::buffer::buffer_get_own_prop(addr, "direct_fresh").is_none(),
        "neither reflected nor direct Set may add a key after preventExtensions"
    );

    // No roots: dead at the full trace. Buffer-backed Uint8Arrays are not
    // `GC_TYPE_TYPED_ARRAY` cells, so only the buffer finalize hook can remove
    // this address-keyed entry.
    full_gc();

    assert!(
        live_buffer_type(addr).is_none(),
        "the owner must really die"
    );
    let fresh = crate::buffer::js_uint8array_alloc(32) as usize;
    assert!(!crate::typedarray_props::typed_array_owner_no_extend(fresh));
    assert_eq!(crate::buffer::buffer_get_own_prop(fresh, "existing"), None);
}

/// A LIVE buffer keeps its own properties across a full collection. Without
/// this the prune above could pass by dropping everything unconditionally.
///
/// `CopyingNurseryTestGuard::new(1)` — not `GcTestIsolationGuard` — because
/// only it pushes the shadow frame that makes `js_shadow_slot_set` an actual
/// root; see the note on the DataView/SAB survival test above.
#[test]
fn test_live_buffer_keeps_its_own_properties_across_full_gc() {
    let _guard = CopyingNurseryTestGuard::new(1);

    let addr = crate::buffer::buffer_alloc(32) as usize;
    crate::buffer::buffer_set_own_prop(addr, "tag", 7.0);
    js_shadow_slot_set(0, ptr_bits(addr));

    full_gc();

    assert_eq!(
        crate::buffer::buffer_get_own_prop(addr, "tag"),
        Some(7.0),
        "a live (rooted) buffer must keep its own properties"
    );
}

/// The leak regression: N property-carrying buffers, all references dropped,
/// one full collection, and the table must DRAIN. A per-address probe cannot
/// show this — before the fix the table grew monotonically for the life of the
/// process.
#[test]
fn test_buffer_property_bags_die_with_their_owners() {
    let _guard = GcTestIsolationGuard::new();
    let owner = crate::buffer::buffer_alloc(32) as usize;
    crate::buffer::buffer_set_own_prop(owner, "tag", 7.0);
    let bag = unsafe { crate::buffer::store::bag(owner) } as usize;
    assert_ne!(bag, 0, "the property must create real storage");
    clear_marks();
    clear_mark_seeds();
    let valid = build_valid_pointer_set();
    mark_mutable_registered_roots(&valid);
    assert_eq!(
        unsafe { (*header_from_user_ptr(bag as *const u8)).gc_flags & GC_FLAG_MARKED },
        0,
        "no root may retain the property bag after its owner dies"
    );
    clear_marks();
    clear_mark_seeds();
    full_gc();
    assert!(live_buffer_type(owner).is_none());
}

/// The invariant that makes every address-keyed buffer registry legitimate in
/// the first place: a `BufferHeader` is never relocated, so its address is a
/// stable key for the object's whole lifetime and only DEATH (the pruning the
/// rest of this file tests) can invalidate an entry.
///
/// Nothing pinned this before, and a great deal rests on it: `bun:ffi`'s
/// pointer-lifetime contract hands `ptr(view)` to native code and documents
/// the address as stable for the lifetime of the JS object
/// (`bun_ffi/mod.rs`); the remaining identity registries
/// above are keyed by that address; hoisted emitted data pointers into
/// inline bytes assume it; and #9611 publishes
/// `WebAssembly.Memory.prototype.buffer` as a foreign-backed wrapper whose
/// address the wasm binding table keys. Flipping either type to `movable`
/// invalidates all of them at once, silently — this test is where that shows
/// up instead.
///
/// Movability is enforced in two places, and both are what this asserts
/// through: `gc_type_is_movable` gates the per-object and the block-granular
/// old-page evacuation paths (`gc/oldgen.rs`), and buffers are born in the old
/// arena as `TENURED`, so no copying minor ever sees one either.
#[test]
fn test_buffer_headers_are_never_relocated() {
    assert!(
        !crate::gc::types::gc_type_is_movable(crate::gc::GC_TYPE_BUFFER),
        "GC_TYPE_BUFFER must stay non-movable: every buffer registry is keyed \
         by the header address, and bun:ffi hands that address to native code"
    );
    assert!(
        !crate::gc::types::gc_type_is_movable(crate::gc::GC_TYPE_TYPED_ARRAY),
        "GC_TYPE_TYPED_ARRAY must stay non-movable for the same reason"
    );
}

/// A moving nursery collection must preserve a view reachable through a
/// relocated holder. A subsequent full trace must retain the original backing
/// and stable .buffer identity with no direct roots to either. Once the holder
/// dies, the view/backing/identity cycle must be reclaimed.
#[test]
fn test_buffer_subarray_owner_edges_survive_moving_gc_and_die_together() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let source = crate::buffer::js_uint8array_alloc(64);
    crate::buffer::js_buffer_set(source, 18, 42);
    let intermediate = crate::buffer::js_buffer_slice(source, 8, 48);
    let view = crate::buffer::js_buffer_slice(intermediate, 10, 20);
    let identity = crate::buffer::buffer_backing_array_buffer(view as usize);
    let holder = crate::array::js_array_alloc(1);
    crate::array::js_array_push_f64(holder, f64::from_bits(ptr_bits(view as usize)));
    assert!(crate::arena::pointer_in_nursery(holder as usize));
    js_shadow_slot_set(0, ptr_bits(holder as usize));

    let _ = gc_collect_minor();
    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(moved, holder as usize, "the holder must actually move");
    full_gc();
    assert!(crate::buffer::is_registered_buffer(source as usize));
    assert!(crate::buffer::is_registered_buffer(identity));
    assert!(crate::buffer::is_registered_buffer(view as usize));
    assert!(
        live_buffer_type(intermediate as usize).is_none(),
        "nested views retain the ultimate backing, not the intermediate receiver"
    );
    assert_eq!(crate::buffer::js_buffer_get(view, 0), 42);
    assert_eq!(
        crate::buffer::buffer_backing_array_buffer(view as usize),
        identity
    );
    crate::buffer::js_buffer_set(view, 0, 91);
    assert_eq!(crate::buffer::js_buffer_get(source, 18), 91);

    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
    full_gc();
    for addr in [source as usize, view as usize, identity] {
        assert!(
            live_buffer_type(addr).is_none(),
            "dead backing cycle retained {addr:#x}"
        );
        assert!(crate::buffer::view::lookup(addr).is_none());
    }
}
