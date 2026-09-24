use super::*;
use crate::dyn_eval::*;

fn closure_fn_id(closure: f64) -> u32 {
    crate::closure::js_closure_get_capture_f64(
        crate::value::js_nanbox_get_pointer(closure) as *const _,
        0,
    ) as u32
}

fn call0(function: f64) -> f64 {
    unsafe { crate::closure::js_native_call_value(function, std::ptr::null(), 0) }
}

/// Disable the source cache (its byte cap is "full") for one test body, so
/// every prepared function is reclaimable.
struct SourceCacheFull;

impl SourceCacheFull {
    fn new() -> Self {
        SOURCE_FN_CACHE_BYTES.with(|b| b.set(SOURCE_FN_CACHE_MAX_BYTES));
        Self
    }
}

impl Drop for SourceCacheFull {
    fn drop(&mut self) {
        SOURCE_FN_CACHE_BYTES.with(|b| b.set(0));
    }
}

#[test]
fn registry_reclaims_overflow_and_preserves_cached_live_and_active_functions() {
    // Fill the real entry-count cap; every following source is uncached.
    for i in 0..SOURCE_FN_CACHE_MAX {
        prepare_function_args(&[format!("return {i}; // fill registry cache")]);
    }
    assert_eq!(
        SOURCE_FN_CACHE.with(|c| c.borrow().len()),
        SOURCE_FN_CACHE_MAX
    );
    let cached = prepare_function_args(&["return 0; // fill registry cache".into()]);
    let active_id = prepare_function_args(&["return 99; // active uncached".into()]);
    let active = lookup_fn(active_id).unwrap();
    let live = dyn_function_from_strings(&["return () => 42; // escaped uncached".into()]);
    let live_idx = root_push(live);
    let nested = call0(live);
    let nested_idx = root_push(nested);
    let owner = crate::value::js_nanbox_get_pointer(nested) as usize;
    let nested_id = closure_fn_id(nested);
    let parent_id = closure_fn_id(live);
    let parent_weak = Rc::downgrade(&lookup_fn(parent_id).unwrap());
    for i in 0..512 {
        prepare_function_args(&[format!("return {i}; // uncached churn")]);
    }
    prune_dead_function_owners(&|addr| addr != owner);
    assert!(
        parent_weak.upgrade().is_none(),
        "dead parent AST must be released"
    );
    assert_eq!(node_cache_len(), 0, "a freed parent takes its node entries");
    assert!(lookup_fn(cached).is_some());
    assert!(lookup_fn(active_id).is_some());
    assert!(lookup_fn(nested_id).is_some());
    assert_eq!(
        FN_REGISTRY.with(|r| r.borrow().len()),
        SOURCE_FN_CACHE_MAX + 2
    );
    assert_eq!(
        call0(root_get(nested_idx)),
        42.0,
        "escaped nested closure survives its parent AST"
    );
    drop(active);
    roots_truncate(live_idx);
    prune_dead_function_owners(&|_| true);
    assert!(lookup_fn(active_id).is_none());
    assert!(lookup_fn(nested_id).is_none());
    assert_eq!(FN_REGISTRY.with(|r| r.borrow().len()), SOURCE_FN_CACHE_MAX);
}

#[test]
fn registry_keeps_nested_functions_while_their_parent_ast_lives() {
    let _uncached = SourceCacheFull::new();
    let parent = dyn_function_from_strings(&["return function inner() { return 42; };".into()]);
    let parent_root = root_push(parent);
    let parent_owner = crate::value::js_nanbox_get_pointer(parent) as usize;
    let parent_id = closure_fn_id(parent);
    let first_id = closure_fn_id(call0(parent));
    // Every nested closure dies (the per-request shape): the parent lives, so
    // neither the child AST nor its node entry may be dropped or rebuilt.
    prune_dead_function_owners(&|owner| owner != parent_owner);
    assert!(lookup_fn(first_id).is_some());
    assert_eq!(node_cache_len(), 1);
    let second = call0(root_get(parent_root));
    assert_eq!(
        closure_fn_id(second),
        first_id,
        "a live parent reuses its nested registration"
    );
    assert_eq!(call0(second), 42.0);
    // Once the parent dies, parent, child and node entry go in one prune.
    roots_truncate(parent_root);
    prune_dead_function_owners(&|_| true);
    assert!(lookup_fn(parent_id).is_none());
    assert!(lookup_fn(first_id).is_none());
    assert_eq!(node_cache_len(), 0);
}

#[test]
fn registry_scopes_script_nested_functions_to_the_script_ast() {
    // vm.runInThisContext("(function(){return 1})") then "…return 2…": the
    // second parse can reuse the first's freed `Function` allocation, so a
    // node cache keyed by that address alone returns the first function.
    let global = crate::object::js_get_global_this();
    let lexical = script_environment(global, &[]);
    let lexical_idx = root_push(lexical);
    let first = eval_script_in(
        "(function(){return 1})",
        global,
        global,
        root_get(lexical_idx),
    );
    let first_idx = root_push(first);
    let second = eval_script_in(
        "(function(){return 2})",
        global,
        global,
        root_get(lexical_idx),
    );
    let second_idx = root_push(second);
    assert_eq!(call0(root_get(first_idx)), 1.0);
    assert_eq!(
        call0(root_get(second_idx)),
        2.0,
        "the second script must not resolve to the first script's function"
    );
    // The scripts finished: their ASTs and node entries are reclaimable while
    // the escaped closures keep their own (cloned) function ASTs.
    let (first_owner, second_owner) = (
        crate::value::js_nanbox_get_pointer(root_get(first_idx)) as usize,
        crate::value::js_nanbox_get_pointer(root_get(second_idx)) as usize,
    );
    prune_dead_function_owners(&|owner| owner != first_owner && owner != second_owner);
    assert_eq!(node_cache_len(), 0, "finished scripts release node entries");
    assert_eq!(call0(root_get(first_idx)), 1.0);
    assert_eq!(call0(root_get(second_idx)), 2.0);
    roots_truncate(lexical_idx);
}

#[test]
fn registry_releases_active_ast_pins_after_a_throw() {
    let _uncached = SourceCacheFull::new();
    let base = roots_len();
    let closure = dyn_function_from_strings(&["throw 7;".into()]);
    assert_eq!(roots_len(), base, "construction must release its pins");
    let id = closure_fn_id(closure);
    let weak = Rc::downgrade(&lookup_fn(id).unwrap());
    let result = crate::exception::catch_js_throw(|| call0(closure));
    assert!(result.is_err());
    assert_eq!(roots_len(), base);
    assert!(ACTIVE_FUNCTIONS.with(|pins| pins.borrow().is_empty()));
    assert_eq!(PIN_TOP.with(Cell::get), 0);
    prune_dead_function_owners(&|_| true);
    assert!(weak.upgrade().is_none(), "throw must not leak a cloned Rc");
}

#[test]
fn registry_reclaims_when_source_byte_cap_is_full_or_cache_is_disabled() {
    // Exercise the byte-limit arm without retaining a 32 MiB fixture.
    let uncached = SourceCacheFull::new();
    let id = prepare_function_args(&["return 17; // byte cap".into()]);
    assert!(SOURCE_FN_CACHE.with(|c| c.borrow().is_empty()));
    let weak = Rc::downgrade(&lookup_fn(id).unwrap());
    prune_dead_function_owners(&|_| true);
    assert!(weak.upgrade().is_none());
    drop(uncached);
    // prepare_source is the exact no-parse-cache branch.
    let id = prepare_source("(function() { return 23; })");
    prune_dead_function_owners(&|_| true);
    assert!(lookup_fn(id).is_none());
}

#[test]
fn registry_ids_wrap_without_reusing_a_registered_id() {
    let empty = || InterpFn {
        params: Vec::new(),
        body: InterpBody::Block(Vec::new()),
        hoisted_vars: Vec::new(),
        strict: false,
    };
    let held = register_fn(empty());
    let held_pin = pin_function(held).unwrap();
    NEXT_FN_ID.with(|next| next.set(u32::MAX));
    assert_eq!(register_fn(empty()), u32::MAX);
    NEXT_FN_ID.with(|next| next.set(held));
    let wrapped = register_fn(empty());
    assert_ne!(wrapped, held, "a live id must never be handed out again");
    assert_ne!(wrapped, 0);
    drop(held_pin);
    prune_dead_function_owners(&|_| true);
}

#[test]
fn registry_node_cache_never_crosses_parent_asts_at_a_reused_address() {
    // Deterministic form of the address-reuse hazard: two different parent
    // ASTs whose nested function nodes land at the SAME address (the second
    // parse reusing the first's freed allocation) must not share an entry.
    let parent = || InterpFn {
        params: Vec::new(),
        body: InterpBody::Block(Vec::new()),
        hoisted_vars: Vec::new(),
        strict: false,
    };
    let first_parent = register_fn(parent());
    let second_parent = register_fn(parent());
    const REUSED_ADDR: usize = 0x1000;
    let first = node_fn_id(first_parent, REUSED_ADDR, parent);
    let second = node_fn_id(second_parent, REUSED_ADDR, parent);
    assert_ne!(first, second, "a node key must include its parent AST");
    assert_eq!(node_fn_id(first_parent, REUSED_ADDR, parent), first);
    prune_dead_function_owners(&|_| true);
    assert!(lookup_fn(first).is_none() && lookup_fn(second).is_none());
    assert_eq!(node_cache_len(), 0);
}
