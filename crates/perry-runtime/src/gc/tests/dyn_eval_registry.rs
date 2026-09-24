//! Real GC integration for the interpreter's weak closure owners.
use super::super::*;
use super::support::*;
use crate::dyn_eval::{lookup_fn, register_closure, register_fn, InterpBody, InterpFn};

fn function_owner(old: bool) -> (usize, u32) {
    let id = register_fn(InterpFn {
        params: Vec::new(),
        body: InterpBody::Block(Vec::new()),
        hoisted_vars: Vec::new(),
        strict: false,
    });
    let size = std::mem::size_of::<crate::closure::ClosureHeader>();
    let align = std::mem::align_of::<crate::closure::ClosureHeader>();
    let owner = if old {
        crate::arena::arena_alloc_gc_old(size, align, GC_TYPE_CLOSURE)
    } else {
        crate::arena::arena_alloc_gc(size, align, GC_TYPE_CLOSURE)
    };
    unsafe { init_test_closure(owner) };
    register_closure(owner as usize, id);
    (owner as usize, id)
}

#[test]
fn dyn_eval_registry_releases_ast_on_full_gc() {
    let _guard = GcTestIsolationGuard::new();
    let (_, id) = function_owner(false);
    let weak = std::rc::Rc::downgrade(&lookup_fn(id).unwrap());
    super::dead_owner_side_tables::full_gc_with_no_block_persistence();
    assert!(lookup_fn(id).is_none());
    assert!(
        weak.upgrade().is_none(),
        "the AST allocation must actually drop"
    );
}

#[test]
fn dyn_eval_registry_follows_copying_survivor_and_reclaims_dead_nursery_owner() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let (live, live_id) = function_owner(false);
    let (_, dead_id) = function_owner(false);
    js_shadow_slot_set(0, ptr_bits(live));
    let _ = gc_collect_minor();
    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(moved, live, "test must move the live closure");
    assert!(lookup_fn(live_id).is_some());
    assert!(lookup_fn(dead_id).is_none());
    // A second collection must see the new owner address, not from-space.
    let _ = gc_collect_minor();
    assert!(lookup_fn(live_id).is_some());
    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
    super::dead_owner_side_tables::full_gc_with_no_block_persistence();
    assert!(lookup_fn(live_id).is_none());
}

#[test]
fn dyn_eval_registry_preserves_old_owner_during_minor() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let (_, id) = function_owner(true);
    let _ = gc_collect_minor();
    assert!(
        lookup_fn(id).is_some(),
        "a minor cannot prove an old owner dead"
    );
    super::dead_owner_side_tables::full_gc_with_no_block_persistence();
    assert!(lookup_fn(id).is_none());
}
