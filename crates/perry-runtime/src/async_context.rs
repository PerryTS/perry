//! AsyncLocalStorage context propagation support.
//!
//! This module owns the thread-local execution context used by
//! `node:async_hooks` AsyncLocalStorage. The stdlib module mutates the active
//! context; async schedulers snapshot it when work is queued and restore it
//! while the callback runs.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::gc::{RuntimeHandle, RuntimeHandleScope};

#[derive(Clone, Default)]
pub struct AsyncContextSnapshot {
    entries: Vec<AsyncContextEntry>,
}

impl AsyncContextSnapshot {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Clone)]
struct AsyncContextEntry {
    handle: i64,
    generation: u64,
    stores: Vec<f64>,
}

thread_local! {
    static ACTIVE_CONTEXT: RefCell<AsyncContextSnapshot> = RefCell::new(AsyncContextSnapshot::default());
    static HANDLE_GENERATIONS: RefCell<HashMap<i64, u64>> = RefCell::new(HashMap::new());
}

fn handle_generation(handle: i64) -> u64 {
    HANDLE_GENERATIONS.with(|generations| generations.borrow().get(&handle).copied().unwrap_or(0))
}

fn discard_disabled_entries(snapshot: &mut AsyncContextSnapshot) {
    snapshot
        .entries
        .retain(|entry| entry.generation == handle_generation(entry.handle));
}

pub fn capture_context() -> AsyncContextSnapshot {
    ACTIVE_CONTEXT.with(|ctx| ctx.borrow().clone())
}

pub fn enter_context(snapshot: &AsyncContextSnapshot) -> AsyncContextSnapshot {
    ACTIVE_CONTEXT.with(|ctx| {
        let mut ctx = ctx.borrow_mut();
        let previous = ctx.clone();
        let mut next = snapshot.clone();
        discard_disabled_entries(&mut next);
        *ctx = next;
        previous
    })
}

pub fn restore_context(mut snapshot: AsyncContextSnapshot) {
    discard_disabled_entries(&mut snapshot);
    ACTIVE_CONTEXT.with(|ctx| {
        *ctx.borrow_mut() = snapshot;
    });
}

// ---------------------------------------------------------------------------
// AsyncLocalStorage provider ABI
// ---------------------------------------------------------------------------
//
// These entry points deliberately keep the active context and its throw guards
// in perry-runtime.  An app-only dylib calls the AsyncLocalStorage methods in a
// separately loaded perry-stdlib provider, while promises, timers and
// microtasks snapshot context in the runtime provider.  Calling this module's
// Rust-private symbols directly from perry-stdlib either makes that provider
// fail eager relocation or makes it link a second runtime whose thread-local
// ACTIVE_CONTEXT is invisible to the schedulers.  The C ABI is therefore the
// ownership boundary: every image resolves these calls to the one runtime
// provider loaded by the host.

/// Enter an `AsyncLocalStorage#run` scope and register its throw-safe restore.
#[no_mangle]
pub extern "C" fn js_async_context_als_run_enter(handle: i64, store: f64) {
    push_store(handle, store);
    push_context_guard(ContextGuardAction::PopStore(handle));
}

/// Enter an `AsyncLocalStorage#exit` scope and register its throw-safe restore.
#[no_mangle]
pub extern "C" fn js_async_context_als_exit_enter(handle: i64) {
    let saved = take_store(handle);
    push_context_guard(ContextGuardAction::RestoreStores(handle, saved));
}

/// Leave the most recently entered ALS `run`/`exit` scope normally.
#[no_mangle]
pub extern "C" fn js_async_context_als_scope_leave() {
    if let Some(action) = pop_context_guard() {
        apply_context_guard(action);
    }
}

/// Return the current store for one ALS instance, or JavaScript `undefined`.
#[no_mangle]
pub extern "C" fn js_async_context_als_get_store(handle: i64) -> f64 {
    get_store(handle).unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED))
}

/// Implement `AsyncLocalStorage#enterWith` in the runtime-owned context.
#[no_mangle]
pub extern "C" fn js_async_context_als_enter_with(handle: i64, store: f64) {
    enter_with(handle, store);
}

/// Remove one ALS instance from the runtime-owned active context.
#[no_mangle]
pub extern "C" fn js_async_context_als_clear(handle: i64) {
    clear_store(handle);
}

pub fn push_store(handle: i64, store: f64) {
    let generation = handle_generation(handle);
    ACTIVE_CONTEXT.with(|ctx| {
        let mut ctx = ctx.borrow_mut();
        ctx.entries
            .retain(|entry| entry.handle != handle || entry.generation == generation);
        if let Some(entry) = ctx
            .entries
            .iter_mut()
            .find(|entry| entry.handle == handle && entry.generation == generation)
        {
            entry.stores.push(store);
        } else {
            ctx.entries.push(AsyncContextEntry {
                handle,
                generation,
                stores: vec![store],
            });
        }
    });
}

pub fn pop_store(handle: i64) {
    let generation = handle_generation(handle);
    ACTIVE_CONTEXT.with(|ctx| {
        let mut ctx = ctx.borrow_mut();
        if let Some(index) = ctx
            .entries
            .iter()
            .position(|entry| entry.handle == handle && entry.generation == generation)
        {
            ctx.entries[index].stores.pop();
            if ctx.entries[index].stores.is_empty() {
                ctx.entries.remove(index);
            }
        }
    });
}

pub fn get_store(handle: i64) -> Option<f64> {
    let generation = handle_generation(handle);
    ACTIVE_CONTEXT.with(|ctx| {
        ctx.borrow()
            .entries
            .iter()
            .find(|entry| entry.handle == handle && entry.generation == generation)
            .and_then(|entry| entry.stores.last().copied())
    })
}

/// Replace the current store for `handle` (top of its stack) without growing
/// the stack, pushing only when the handle has no active store. This is
/// `AsyncLocalStorage#enterWith` semantics: Node's AsyncContextFrame `set`
/// swaps the storage's value in the current frame, so a surrounding `run()`
/// (which saves/restores exactly one slot for its own handle) still restores
/// the pre-`run` value on exit (#788, differential case 21).
pub fn set_store(handle: i64, store: f64) {
    let generation = handle_generation(handle);
    ACTIVE_CONTEXT.with(|ctx| {
        let mut ctx = ctx.borrow_mut();
        ctx.entries
            .retain(|entry| entry.handle != handle || entry.generation == generation);
        if let Some(entry) = ctx
            .entries
            .iter_mut()
            .find(|entry| entry.handle == handle && entry.generation == generation)
        {
            if let Some(slot) = entry.stores.last_mut() {
                *slot = store;
            } else {
                entry.stores.push(store);
            }
        } else {
            ctx.entries.push(AsyncContextEntry {
                handle,
                generation,
                stores: vec![store],
            });
        }
    });
}

pub fn enter_with(handle: i64, store: f64) {
    set_store(handle, store);
}

/// Deferred context-restore action for a scope (`AsyncLocalStorage#run`/
/// `#exit`, `AsyncResource#runInAsyncScope`) whose callback may throw.
/// `js_throw` longjmps past the normal restore code, so each scope records
/// its restore action here (tagged with the `try` depth at entry) and
/// `js_throw` applies every action belonging to frames it is about to
/// unwind past (#788, differential cases 10/25).
pub enum ContextGuardAction {
    /// `run()`: pop the one store slot the scope pushed for its handle.
    PopStore(i64),
    /// `exit()`: restore the handle's store stack removed at entry.
    RestoreStores(i64, Option<(u64, Vec<f64>)>),
    /// `runInAsyncScope()` / snapshot trampoline: restore the full snapshot.
    RestoreSnapshot(AsyncContextSnapshot),
    /// Silently pop one async_hooks execution-id frame (no `after` hook
    /// callbacks: arbitrary JS must not run mid-`js_throw`).
    RestoreExecutionIds,
}

struct ContextGuard {
    try_depth: usize,
    action: ContextGuardAction,
}

thread_local! {
    static CONTEXT_GUARDS: RefCell<Vec<ContextGuard>> = const { RefCell::new(Vec::new()) };
}

pub fn push_context_guard(action: ContextGuardAction) {
    let try_depth = crate::exception::current_try_depth();
    CONTEXT_GUARDS.with(|guards| {
        guards.borrow_mut().push(ContextGuard { try_depth, action });
    });
}

/// Pop the most recent guard without applying it. The caller either applies
/// it (normal scope exit) or discards it (the restore already happened by
/// other means).
pub fn pop_context_guard() -> Option<ContextGuardAction> {
    CONTEXT_GUARDS.with(|guards| guards.borrow_mut().pop().map(|guard| guard.action))
}

pub fn apply_context_guard(action: ContextGuardAction) {
    match action {
        ContextGuardAction::PopStore(handle) => pop_store(handle),
        ContextGuardAction::RestoreStores(handle, stores) => restore_store(handle, stores),
        ContextGuardAction::RestoreSnapshot(snapshot) => restore_context(snapshot),
        ContextGuardAction::RestoreExecutionIds => crate::async_hooks::unwind_execution_scope(),
    }
}

/// Called from `js_throw` just before the longjmp: apply (newest-first) every
/// guard registered at a `try` depth greater than the depth of the handler
/// being jumped to — those scopes' normal restore code is being unwound past.
/// Guards registered at or below the handler's depth belong to still-live
/// scopes and stay.
pub(crate) fn unwind_context_guards(target_try_depth: usize) {
    loop {
        let action = CONTEXT_GUARDS.with(|guards| {
            let mut guards = guards.borrow_mut();
            match guards.last() {
                Some(guard) if guard.try_depth > target_try_depth => {
                    guards.pop().map(|guard| guard.action)
                }
                _ => None,
            }
        });
        match action {
            Some(action) => apply_context_guard(action),
            None => break,
        }
    }
}

fn scan_context_guard_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    CONTEXT_GUARDS.with(|guards| {
        for guard in guards.borrow_mut().iter_mut() {
            match &mut guard.action {
                ContextGuardAction::PopStore(_) | ContextGuardAction::RestoreExecutionIds => {}
                ContextGuardAction::RestoreStores(_, stores) => {
                    if let Some((_, stores)) = stores {
                        for store in stores.iter_mut() {
                            visitor.visit_nanbox_f64_slot(store);
                        }
                    }
                }
                ContextGuardAction::RestoreSnapshot(snapshot) => {
                    scan_snapshot_roots_mut(snapshot, visitor);
                }
            }
        }
    });
}

pub fn clear_store(handle: i64) {
    // `disable()` invalidates descendants captured from a currently-active
    // store, but Node leaves already-captured work alone when the storage is
    // disabled after its `run()` scope has returned.  Generation-bump only in
    // the former case so another ALS in the same pending snapshot is not
    // disturbed either.
    let was_active = ACTIVE_CONTEXT.with(|ctx| {
        ctx.borrow()
            .entries
            .iter()
            .any(|entry| entry.handle == handle)
    }) || CONTEXT_GUARDS.with(|guards| {
        guards.borrow().iter().any(|guard| {
            matches!(
                &guard.action,
                ContextGuardAction::RestoreStores(saved_handle, Some(_))
                    if *saved_handle == handle
            )
        })
    });
    if was_active {
        HANDLE_GENERATIONS.with(|generations| {
            let mut generations = generations.borrow_mut();
            let generation = generations.entry(handle).or_insert(0);
            *generation = generation.wrapping_add(1);
        });
    }
    remove_store(handle);
}

fn remove_store(handle: i64) {
    ACTIVE_CONTEXT.with(|ctx| {
        ctx.borrow_mut()
            .entries
            .retain(|entry| entry.handle != handle);
    });
}

pub fn take_store(handle: i64) -> Option<(u64, Vec<f64>)> {
    let generation = handle_generation(handle);
    ACTIVE_CONTEXT.with(|ctx| {
        let mut ctx = ctx.borrow_mut();
        ctx.entries
            .iter()
            .position(|entry| entry.handle == handle && entry.generation == generation)
            .map(|index| {
                let entry = ctx.entries.remove(index);
                (entry.generation, entry.stores)
            })
    })
}

/// Restore a previously removed store stack for one ALS handle.
///
/// `take_store` returns `Some` only for an existing entry, and live entries are
/// kept non-empty by `pop_store`. The empty guard below is defensive for manual
/// callers and prevents inert context entries from accumulating.
pub fn restore_store(handle: i64, stores: Option<(u64, Vec<f64>)>) {
    remove_store(handle);
    if let Some((generation, stores)) = stores {
        if !stores.is_empty() && generation == handle_generation(handle) {
            ACTIVE_CONTEXT.with(|ctx| {
                ctx.borrow_mut().entries.push(AsyncContextEntry {
                    handle,
                    generation,
                    stores,
                });
            });
        }
    }
}

pub fn scan_snapshot_roots(snapshot: &AsyncContextSnapshot, mark: &mut dyn FnMut(f64)) {
    for entry in &snapshot.entries {
        for &store in &entry.stores {
            mark(store);
        }
    }
}

pub fn scan_snapshot_roots_mut(
    snapshot: &mut AsyncContextSnapshot,
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    for entry in &mut snapshot.entries {
        for store in &mut entry.stores {
            visitor.visit_nanbox_f64_slot(store);
        }
    }
}

pub(crate) fn scan_snapshot_roots_mut_step(
    snapshot: &mut AsyncContextSnapshot,
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    entry_cursor: &mut usize,
    store_cursor: &mut usize,
    remaining: &mut usize,
) -> bool {
    while *remaining > 0 && *entry_cursor < snapshot.entries.len() {
        let entry = &mut snapshot.entries[*entry_cursor];
        while *remaining > 0 && *store_cursor < entry.stores.len() {
            visitor.visit_nanbox_f64_slot(&mut entry.stores[*store_cursor]);
            *store_cursor += 1;
            *remaining -= 1;
        }
        if *store_cursor < entry.stores.len() {
            return false;
        }
        *entry_cursor += 1;
        *store_cursor = 0;
    }
    *entry_cursor >= snapshot.entries.len()
}

pub fn scan_active_context_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    ACTIVE_CONTEXT.with(|ctx| {
        scan_snapshot_roots_mut(&mut ctx.borrow_mut(), &mut visitor);
    });
    scan_context_guard_roots_mut(&mut visitor);
}

pub fn scan_active_context_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    ACTIVE_CONTEXT.with(|ctx| {
        scan_snapshot_roots_mut(&mut ctx.borrow_mut(), visitor);
    });
    scan_context_guard_roots_mut(visitor);
}

pub struct AsyncContextSnapshotRoots<'scope> {
    stores: Vec<RuntimeHandle<'scope>>,
}

pub fn root_snapshot<'scope>(
    scope: &'scope RuntimeHandleScope,
    snapshot: &AsyncContextSnapshot,
) -> AsyncContextSnapshotRoots<'scope> {
    let stores = snapshot
        .entries
        .iter()
        .flat_map(|entry| entry.stores.iter())
        .map(|store| scope.root_nanbox_f64(*store))
        .collect();
    AsyncContextSnapshotRoots { stores }
}

pub fn refresh_snapshot_from_roots(
    snapshot: &mut AsyncContextSnapshot,
    roots: &AsyncContextSnapshotRoots<'_>,
) {
    let mut handles = roots.stores.iter();
    for entry in &mut snapshot.entries {
        for store in &mut entry.stores {
            if let Some(handle) = handles.next() {
                *store = handle.get_nanbox_f64();
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn test_snapshot_with_store(store: f64) -> AsyncContextSnapshot {
    AsyncContextSnapshot {
        entries: vec![AsyncContextEntry {
            handle: -1,
            generation: handle_generation(-1),
            stores: vec![store],
        }],
    }
}

#[cfg(test)]
pub(crate) fn test_snapshot_first_store(snapshot: &AsyncContextSnapshot) -> Option<f64> {
    snapshot
        .entries
        .first()
        .and_then(|entry| entry.stores.first().copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn als_provider_abi_keeps_nested_run_and_exit_scopes_balanced() {
        let handle = -8037;
        js_async_context_als_clear(handle);
        js_async_context_als_enter_with(handle, 1.0);

        js_async_context_als_run_enter(handle, 2.0);
        assert_eq!(js_async_context_als_get_store(handle), 2.0);
        js_async_context_als_scope_leave();
        assert_eq!(js_async_context_als_get_store(handle), 1.0);

        js_async_context_als_exit_enter(handle);
        assert_eq!(
            js_async_context_als_get_store(handle).to_bits(),
            crate::value::TAG_UNDEFINED
        );
        js_async_context_als_scope_leave();
        assert_eq!(js_async_context_als_get_store(handle), 1.0);

        js_async_context_als_clear(handle);
        assert_eq!(
            js_async_context_als_get_store(handle).to_bits(),
            crate::value::TAG_UNDEFINED
        );
    }
}
