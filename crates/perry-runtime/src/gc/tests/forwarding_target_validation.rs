//! #8174 — a forwarding walk must not step out of, or into, a non-object.
//!
//! `GC_FLAG_FORWARDED` means "the first payload word is where this object moved
//! to". The two forwarding walkers (`CopyingNurseryCollector::rewrite_raw_addr`
//! and `gc::verify::try_rewrite_raw_addr`) used to trust that byte for any
//! address in a known heap region, and to trust whatever word it found.
//!
//! For a slot the collector already proved is a live reference, both are safe.
//! For a **metadata key** — `RuntimeRootVisitor::visit_metadata_*`, rewritten
//! but deliberately not marked — neither is: the key's object can die, the
//! arena recycles the address, and the recycled payload bytes get read as a
//! `GcHeader`. #8040 is that, observed: `gc_flags = 0x86` (`GC_FLAG_FORWARDED`
//! set by coincidence), `obj_type = 104`, and a "forwarding pointer" that was
//! really a NaN-boxed value. The walk stopped one hop later because the word
//! did not classify — and RETURNED it. `visit_metadata_nanbox_key` masked it to
//! 48 bits and got a live, unrelated survivor.
//!
//! These cases plant that exact shape. Each SABOTAGE case first asserts the
//! premise — that the gate the old code used would have accepted the planted
//! bytes — so a green run says the discriminator works, not that nothing was
//! tried.

use super::super::*;
use super::support::*;

/// #8040's observed word: a NaN-boxed value sitting where a forwarding pointer
/// would be. `0x7FFF…` is above `is_valid_obj_ptr`'s `HEAP_MAX`, so it is not
/// an address at all — but 48-bit masking turns it into one.
const NAN_BOXED_NON_ADDRESS: u64 = crate::value::STRING_TAG | 0x0000_1234_5678_9AB0;

/// THE PREMISE for everything below: a REAL evacuation still rewrites.
///
/// Tightening a forwarding walk is only a fix if the legitimate walk survives
/// it. `rewrite_raw_addr`'s own doc records the opposite mistake — gating on
/// `self.ptrs.classify()` left genuine from-space `shapes.entries` keys
/// un-rekeyed and turned the verifier red. So this case runs first and asserts
/// both that the rewrite happens and that neither refusal counter moved.
#[test]
fn a_real_forwarding_pointer_is_still_followed() {
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let hdr = unsafe { header_from_user_ptr(nursery_user) as *mut GcHeader };

    let sources = refused_forwarding_source_count();
    let targets = refused_forwarding_target_count();
    unsafe {
        set_forwarding_address(hdr, old_user);
    }

    let mut key = nursery_user as usize;
    let rewrote = RuntimeRootVisitor::for_rewrite(&valid_ptrs).visit_metadata_usize_slot(&mut key);

    unsafe {
        (*hdr).gc_flags &= !GC_FLAG_FORWARDED;
    }

    assert!(rewrote, "a genuinely evacuated key must still be rekeyed");
    assert_eq!(key, old_user as usize);
    assert_eq!(
        refused_forwarding_source_count(),
        sources,
        "a real arena object header must not be refused as a walk source"
    );
    assert_eq!(
        refused_forwarding_target_count(),
        targets,
        "a real to-space object must not be refused as a walk target"
    );
}

/// SABOTAGE: the forwarding word is a NaN-boxed value, not an address.
///
/// Before #8174 the walk returned it and the caller masked it to 48 bits. Now
/// the whole rewrite is refused, so the slot keeps the only value that is not a
/// lie about where the object is.
#[test]
fn a_nan_boxed_forwarding_target_is_refused() {
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    let hdr = unsafe { header_from_user_ptr(nursery_user) as *mut GcHeader };

    // PREMISE: 48-bit masking would have produced a plausible heap address, so
    // the old code's silent acceptance was not obviously wrong at the call site.
    let masked = (NAN_BOXED_NON_ADDRESS & crate::value::POINTER_MASK) as usize;
    assert!(
        crate::value::addr_class::is_plausible_heap_addr(masked) || masked < 0x0001_0000_0000_0000,
        "premise: the masked word looks like an address"
    );

    let before = refused_forwarding_target_count();
    unsafe {
        set_forwarding_address(hdr, NAN_BOXED_NON_ADDRESS as *mut u8);
    }

    let mut key = nursery_user as usize;
    let rewrote = RuntimeRootVisitor::for_rewrite(&valid_ptrs).visit_metadata_usize_slot(&mut key);

    unsafe {
        (*hdr).gc_flags &= !GC_FLAG_FORWARDED;
    }

    assert!(!rewrote, "a non-address forwarding target must not rewrite");
    assert_eq!(
        key, nursery_user as usize,
        "the key must be left alone, not bound to a masked NaN-boxed word"
    );
    assert_eq!(
        refused_forwarding_target_count(),
        before + 1,
        "the refusal must be counted, or a production run cannot report it"
    );
}

/// SABOTAGE: the recycled bytes at a dead key, read as a `GcHeader`.
///
/// This is #8040 one step earlier than the case above — the walk should never
/// have reached the forwarding word at all. The planted bytes are the ones the
/// #8168 investigation observed (`gc_flags = 0x86`, `obj_type = 104`), and the
/// forwarding word planted behind them names a genuinely live object, which is
/// what made the old behaviour corrupt rather than merely useless.
#[test]
fn recycled_bytes_are_not_read_as_a_forwarding_stub() {
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    // A live allocation whose payload interior is a legitimate arena address
    // that is NOT an object start — exactly what a recycled key names.
    let owner = crate::arena::arena_alloc_gc(128, 8, GC_TYPE_OBJECT);
    let stale_key = owner as usize + 64;
    let fake_header = (stale_key - GC_HEADER_SIZE) as *mut GcHeader;
    unsafe {
        (*fake_header).obj_type = 104;
        (*fake_header).gc_flags = 0x86;
        (*fake_header)._reserved = 0;
        (*fake_header).size = 48;
        *(stale_key as *mut u64) = owner as u64;
    }

    // PREMISE, in two parts, both of which the pre-#8174 gate consulted and
    // nothing else: the address is in a known heap region, and the byte at the
    // `gc_flags` offset carries `GC_FLAG_FORWARDED`. A walk gated only on those
    // two follows this.
    assert_ne!(
        crate::arena::classify_heap_space(stale_key - GC_HEADER_SIZE),
        crate::arena::HeapSpace::Unknown,
        "premise: the recycled address classifies as heap"
    );
    assert_ne!(
        unsafe { (*fake_header).gc_flags } & GC_FLAG_FORWARDED,
        0,
        "premise: the recycled bytes present as forwarded"
    );
    assert!(
        gc_type_info(104).is_none(),
        "premise: 104 is not a registered GC type, which is what makes these \
         bytes distinguishable from an object"
    );

    let before = refused_forwarding_source_count();
    assert!(
        forwarding_walk_header(stale_key).is_none(),
        "a header with an unregistered obj_type must not be walked"
    );
    assert_eq!(refused_forwarding_source_count(), before + 1);
}

/// The target predicate on its own, over the three shapes a bogus word takes.
#[test]
fn a_forwarding_target_must_be_an_object_start() {
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let live = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    assert!(
        forwarding_target_is_object_start(live as usize),
        "a real arena allocation is a valid forwarding target"
    );
    assert!(
        !forwarding_target_is_object_start(NAN_BOXED_NON_ADDRESS as usize),
        "a NaN-boxed word is above HEAP_MAX and is not an address"
    );
    assert!(
        !forwarding_target_is_object_start(7),
        "a handle-band id is not an address"
    );
    assert!(
        !forwarding_target_is_object_start(0),
        "null is not an address"
    );
}

/// The #8174 registry itself: `gc::dead_owner::fan_out` iterates this array
/// instead of naming its prunes inline, and `scripts/gc_rekeyed_key_tables.py`
/// reads it to adjudicate every `dead_owner:` verdict. A prune silently dropped
/// from it would take a rekeyed table's death story with it — which is exactly
/// how #8040 happened — so assert the shape the gate depends on.
#[test]
fn the_dead_key_prune_registry_keeps_its_shape() {
    use crate::gc::dead_owner::{DeadKeyOwner, DEAD_KEY_PRUNES};

    assert!(
        DEAD_KEY_PRUNES.len() >= 19,
        "DEAD_KEY_PRUNES shrank to {} entries; a prune was removed from the \
         fan-out, and every `dead_owner:` verdict in \
         scripts/gc_rekeyed_key_tables.json that named it is now a lie",
        DEAD_KEY_PRUNES.len()
    );

    let mut labels: Vec<&str> = DEAD_KEY_PRUNES.iter().map(|entry| entry.table).collect();
    let total = labels.len();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(
        labels.len(),
        total,
        "DEAD_KEY_PRUNES table labels must be unique — the registry is an \
         inventory, and two entries with one label hide a table"
    );

    // #8168's table, narrowed to closures. Its absence from the fan-out IS
    // #8040; if a refactor ever drops it again, fail here rather than in a
    // `TypeError: value is not a function` several collections later.
    assert!(
        DEAD_KEY_PRUNES.iter().any(
            |entry| entry.table == "FUNCTION_CLASS_IDS" && entry.owner == DeadKeyOwner::Closure
        ),
        "FUNCTION_CLASS_IDS must stay registered with the GC_TYPE_CLOSURE \
         predicate (#8168)"
    );
}
