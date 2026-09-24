//! Parsed ASTs follow closure liveness, with the bounded source cache as a
//! second owner. Owner addresses are weak: never mark the closures from here.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

crate::perry_thread_local! {
    // Pins live on the interpreter root stack so throws release them even
    // though Perry's exception transport skips Rust destructors.
    static ACTIVE_FUNCTIONS: RefCell<Vec<(usize, Rc<super::InterpFn>)>> = RefCell::new(Vec::new());
    static CLOSURE_FN_IDS: RefCell<HashMap<usize, u32>> = RefCell::new(HashMap::new());
}

/// A borrowed AST backed by an explicit root-stack pin. Ordinary returns
/// truncate through Drop; exception restore truncates the same stack directly.
pub(super) struct FunctionPin {
    function: *const super::InterpFn,
    root: usize,
}

impl std::ops::Deref for FunctionPin {
    type Target = super::InterpFn;

    fn deref(&self) -> &Self::Target {
        // SAFETY: ACTIVE_FUNCTIONS owns the Rc until this pin's root is
        // truncated. Callers finish all AST reads before truncating that root;
        // a throw that truncates it never returns to the abandoned caller.
        unsafe { &*self.function }
    }
}

impl Drop for FunctionPin {
    fn drop(&mut self) {
        super::roots_truncate(self.root);
    }
}

pub(super) fn pin_function(id: u32) -> Option<FunctionPin> {
    let function = super::lookup_fn(id)?;
    let ptr = Rc::as_ptr(&function);
    let root = super::root_push(super::bridge::undefined());
    ACTIVE_FUNCTIONS.with(|pins| pins.borrow_mut().push((root, function)));
    Some(FunctionPin {
        function: ptr,
        root,
    })
}

pub(super) fn release_function_pins(root_len: usize) {
    ACTIVE_FUNCTIONS.with(|pins| {
        let mut pins = pins.borrow_mut();
        while pins.last().is_some_and(|(root, _)| *root >= root_len) {
            pins.pop();
        }
    });
}

pub(crate) fn register_closure(owner: usize, id: u32) {
    CLOSURE_FN_IDS.with(|owners| owners.borrow_mut().insert(owner, id));
}

pub(crate) fn function_owner_moved(old: usize, new: usize) {
    CLOSURE_FN_IDS.with(|owners| {
        let mut owners = owners.borrow_mut();
        if let Some(id) = owners.remove(&old) {
            owners.insert(new, id);
        }
    });
}

/// Called by the GC's dead-owner pass, before dead storage is recycled.
/// Its minor predicate preserves old-generation owners, and its copying
/// predicate preserves survivors already rekeyed by the payload move hook.
pub(crate) fn prune_dead_function_owners(is_dead: &dyn Fn(usize) -> bool) {
    if super::FN_REGISTRY.with(|registry| registry.borrow().is_empty()) {
        return;
    }
    let mut live = CLOSURE_FN_IDS.with(|owners| {
        let mut owners = owners.borrow_mut();
        owners.retain(|owner, _| !is_dead(*owner));
        owners.values().copied().collect::<HashSet<_>>()
    });
    super::SOURCE_FN_CACHE.with(|cache| live.extend(cache.borrow().values().copied()));
    super::FN_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        // A running call or a not-yet-allocated closure holds an Rc across
        // allocations. Its AST must survive even if no closure is reachable.
        let keep = |id: &u32, function: &Rc<super::InterpFn>| {
            live.contains(id) || Rc::strong_count(function) > 1
        };
        if registry.iter().any(|(id, function)| !keep(id, function)) {
            // NODE_FN_IDS keys are addresses inside these ASTs, including
            // parents of escaped nested closures. Invalidate BEFORE freeing
            // any parent so address reuse can never hit stale parse results.
            super::interp::clear_node_fn_cache();
            registry.retain(|id, function| keep(id, function));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dyn_eval::*;

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
        let nested = unsafe { crate::closure::js_native_call_value(live, std::ptr::null(), 0) };
        let nested_idx = root_push(nested);
        let owner = crate::value::js_nanbox_get_pointer(nested) as usize;
        let nested_id = crate::closure::js_closure_get_capture_f64(owner as *const _, 0) as u32;
        let parent_id = crate::closure::js_closure_get_capture_f64(
            crate::value::js_nanbox_get_pointer(live) as *const _,
            0,
        ) as u32;
        let parent_weak = Rc::downgrade(&lookup_fn(parent_id).unwrap());
        for i in 0..512 {
            prepare_function_args(&[format!("return {i}; // uncached churn")]);
        }
        prune_dead_function_owners(&|addr| addr != owner);
        assert!(
            parent_weak.upgrade().is_none(),
            "dead parent AST must be released"
        );
        assert!(lookup_fn(cached).is_some());
        assert!(lookup_fn(active_id).is_some());
        assert!(lookup_fn(nested_id).is_some());
        assert_eq!(
            FN_REGISTRY.with(|r| r.borrow().len()),
            SOURCE_FN_CACHE_MAX + 2
        );
        let result = unsafe {
            crate::closure::js_native_call_value(root_get(nested_idx), std::ptr::null(), 0)
        };
        assert_eq!(
            result, 42.0,
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
    fn registry_rebuilds_nested_function_cache_after_reclamation() {
        let parent = dyn_function_from_strings(&["return function inner() { return 42; };".into()]);
        let parent_root = root_push(parent);
        let parent_owner = crate::value::js_nanbox_get_pointer(parent) as usize;
        let call = |function| unsafe {
            crate::closure::js_native_call_value(function, std::ptr::null(), 0)
        };
        let first = call(parent);
        let first_id = crate::closure::js_closure_get_capture_f64(
            crate::value::js_nanbox_get_pointer(first) as *const _,
            0,
        ) as u32;
        prune_dead_function_owners(&|owner| owner != parent_owner);
        assert!(lookup_fn(first_id).is_none());
        let second = call(root_get(parent_root));
        let second_id = crate::closure::js_closure_get_capture_f64(
            crate::value::js_nanbox_get_pointer(second) as *const _,
            0,
        ) as u32;
        assert_ne!(
            first_id, second_id,
            "the node cache must rebuild reclaimed entries"
        );
        assert_eq!(call(second), 42.0);
        roots_truncate(parent_root);
    }

    #[test]
    fn registry_releases_active_ast_pins_after_a_throw() {
        SOURCE_FN_CACHE_BYTES.with(|b| b.set(SOURCE_FN_CACHE_MAX_BYTES));
        let base = roots_len();
        let closure = dyn_function_from_strings(&["throw 7;".into()]);
        assert_eq!(roots_len(), base, "construction must release its pins");
        let id = crate::closure::js_closure_get_capture_f64(
            crate::value::js_nanbox_get_pointer(closure) as *const _,
            0,
        ) as u32;
        let weak = Rc::downgrade(&lookup_fn(id).unwrap());
        let result = crate::exception::catch_js_throw(|| unsafe {
            crate::closure::js_native_call_value(closure, std::ptr::null(), 0)
        });
        assert!(result.is_err());
        assert_eq!(roots_len(), base);
        assert!(ACTIVE_FUNCTIONS.with(|pins| pins.borrow().is_empty()));
        prune_dead_function_owners(&|_| true);
        assert!(weak.upgrade().is_none(), "throw must not leak a cloned Rc");
        SOURCE_FN_CACHE_BYTES.with(|b| b.set(0));
    }

    #[test]
    fn registry_reclaims_when_source_byte_cap_is_full_or_cache_is_disabled() {
        // Exercise the byte-limit arm without retaining a 32 MiB fixture.
        SOURCE_FN_CACHE_BYTES.with(|b| b.set(SOURCE_FN_CACHE_MAX_BYTES));
        let id = prepare_function_args(&["return 17; // byte cap".into()]);
        assert!(SOURCE_FN_CACHE.with(|c| c.borrow().is_empty()));
        let weak = Rc::downgrade(&lookup_fn(id).unwrap());
        prune_dead_function_owners(&|_| true);
        assert!(weak.upgrade().is_none());
        SOURCE_FN_CACHE_BYTES.with(|b| b.set(0));
        // prepare_source is the exact no-parse-cache branch.
        let id = prepare_source("(function() { return 23; })");
        prune_dead_function_owners(&|_| true);
        assert!(lookup_fn(id).is_none());
    }
}
