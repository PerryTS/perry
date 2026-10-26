//! Parsed functions follow the closure's capture, with no owner registry.
use super::super::*;
use super::support::*;
use crate::codegen_abi::LayoutWord;
use crate::dyn_eval::{lookup_fn, register_fn, InterpBody, InterpFn};
extern "C" fn body(_: *const crate::closure::ClosureHeader, _: crate::closure::JsThis) -> f64 {
    0.0
}
fn function_owner(old: bool) -> (usize, f64) {
    let id = register_fn(InterpFn {
        params: Vec::new(),
        body: InterpBody::Block(Vec::new()),
        hoisted_vars: Vec::new(),
        strict: false,
        node_functions: Default::default(),
    });
    let size = std::mem::size_of::<crate::closure::ClosureHeader>() + 8;
    let owner = if old {
        crate::arena::arena_alloc_gc_old(size, 8, GC_TYPE_CLOSURE)
    } else {
        crate::arena::arena_alloc_gc(size, 8, GC_TYPE_CLOSURE)
    }
    .cast::<crate::closure::ClosureHeader>();
    unsafe {
        std::ptr::write(
            owner,
            crate::closure::ClosureHeader {
                capture_count: 1,
                shape_id: LayoutWord(0),
                info: crate::fn_info!(body, 0),
                props: std::ptr::null_mut(),
            },
        );
        crate::closure::js_closure_set_capture_f64(owner, 0, id);
    }
    (owner as usize, id)
}
#[test]
fn interpreted_function_cell_survives_copying_owner_and_dies_with_owner() {
    check(false);
}
#[test]
#[ignore = "negative control: remove the closure's parsed-function capture"]
fn negative_control_objtables_dyn_eval_owners() {
    check(true);
}
fn check(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    let (live, id) = function_owner(false);
    let weak = std::rc::Rc::downgrade(&lookup_fn(id).unwrap());
    js_shadow_slot_set(0, ptr_bits(live));
    let _ = gc_collect_minor();
    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::closure::ClosureHeader;
    assert_ne!(moved as usize, live);
    if erase {
        crate::closure::js_closure_set_capture_f64(
            moved,
            0,
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
    }
    js_gc_collect();
    assert!(
        weak.upgrade().is_some(),
        "only the closure's capture owns this uncached AST"
    );
    assert!(lookup_fn(id).is_some());
    let (_, fresh) = function_owner(false);
    assert!(!std::rc::Rc::ptr_eq(
        &lookup_fn(id).unwrap(),
        &lookup_fn(fresh).unwrap()
    ));
    js_shadow_slot_set(0, 0);
    js_gc_collect();
    assert!(
        weak.upgrade().is_none(),
        "no function or lifetime registry retains a dead AST"
    );
    assert!(lookup_fn(id).is_none());
}
#[test]
fn interpreted_function_old_owner_is_retained_until_full_trace() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    let (_, id) = function_owner(true);
    let weak = std::rc::Rc::downgrade(&lookup_fn(id).unwrap());
    let _ = gc_collect_minor();
    assert!(
        weak.upgrade().is_some(),
        "a minor cannot prove an old owner dead"
    );
    js_gc_collect();
    assert!(weak.upgrade().is_none());
}

#[test]
fn interpreted_function_cell_releases_its_ast_at_malloc_teardown() {
    let _guard = GcTestIsolationGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let (owner, id) = function_owner(false);
    let weak = std::rc::Rc::downgrade(&lookup_fn(id).unwrap());
    crate::closure::js_closure_set_capture_f64(
        owner as *mut crate::closure::ClosureHeader,
        0,
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
    unsafe {
        pin_user_ptr_non_young((id.to_bits() & POINTER_MASK) as *mut u8);
        super::super::malloc::MallocState::retire_interp_cell_via_teardown(
            (id.to_bits() & POINTER_MASK) as *mut u8,
        );
    }
    assert!(
        weak.upgrade().is_none(),
        "worker teardown must release the native AST"
    );
}
