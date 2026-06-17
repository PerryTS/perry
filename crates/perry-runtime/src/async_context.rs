//! AsyncLocalStorage context propagation support.
//!
//! This module owns the thread-local execution context used by
//! `node:async_hooks` AsyncLocalStorage. The stdlib module mutates the active
//! context; async schedulers snapshot it when work is queued and restore it
//! while the callback runs.

use std::cell::RefCell;

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
    stores: Vec<f64>,
}

thread_local! {
    static ACTIVE_CONTEXT: RefCell<AsyncContextSnapshot> = RefCell::new(AsyncContextSnapshot::default());
}

pub fn capture_context() -> AsyncContextSnapshot {
    ACTIVE_CONTEXT.with(|ctx| ctx.borrow().clone())
}

pub fn enter_context(snapshot: &AsyncContextSnapshot) -> AsyncContextSnapshot {
    ACTIVE_CONTEXT.with(|ctx| {
        let mut ctx = ctx.borrow_mut();
        let previous = ctx.clone();
        *ctx = snapshot.clone();
        previous
    })
}

pub fn restore_context(snapshot: AsyncContextSnapshot) {
    ACTIVE_CONTEXT.with(|ctx| {
        *ctx.borrow_mut() = snapshot;
    });
}

pub fn push_store(handle: i64, store: f64) {
    ACTIVE_CONTEXT.with(|ctx| {
        let mut ctx = ctx.borrow_mut();
        if let Some(entry) = ctx.entries.iter_mut().find(|entry| entry.handle == handle) {
            entry.stores.push(store);
        } else {
            ctx.entries.push(AsyncContextEntry {
                handle,
                stores: vec![store],
            });
        }
    });
}

pub fn pop_store(handle: i64) {
    ACTIVE_CONTEXT.with(|ctx| {
        let mut ctx = ctx.borrow_mut();
        if let Some(index) = ctx.entries.iter().position(|entry| entry.handle == handle) {
            ctx.entries[index].stores.pop();
            if ctx.entries[index].stores.is_empty() {
                ctx.entries.remove(index);
            }
        }
    });
}

pub fn get_store(handle: i64) -> Option<f64> {
    ACTIVE_CONTEXT.with(|ctx| {
        ctx.borrow()
            .entries
            .iter()
            .find(|entry| entry.handle == handle)
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
    ACTIVE_CONTEXT.with(|ctx| {
        let mut ctx = ctx.borrow_mut();
        if let Some(entry) = ctx.entries.iter_mut().find(|entry| entry.handle == handle) {
            if let Some(slot) = entry.stores.last_mut() {
                *slot = store;
            } else {
                entry.stores.push(store);
            }
        } else {
            ctx.entries.push(AsyncContextEntry {
                handle,
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
    RestoreStores(i64, Option<Vec<f64>>),
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
                    if let Some(stores) = stores {
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
    ACTIVE_CONTEXT.with(|ctx| {
        ctx.borrow_mut()
            .entries
            .retain(|entry| entry.handle != handle);
    });
}

pub fn take_store(handle: i64) -> Option<Vec<f64>> {
    ACTIVE_CONTEXT.with(|ctx| {
        let mut ctx = ctx.borrow_mut();
        ctx.entries
            .iter()
            .position(|entry| entry.handle == handle)
            .map(|index| ctx.entries.remove(index).stores)
    })
}

/// Restore a previously removed store stack for one ALS handle.
///
/// `take_store` returns `Some` only for an existing entry, and live entries are
/// kept non-empty by `pop_store`. The empty guard below is defensive for manual
/// callers and prevents inert context entries from accumulating.
pub fn restore_store(handle: i64, stores: Option<Vec<f64>>) {
    clear_store(handle);
    if let Some(stores) = stores {
        if !stores.is_empty() {
            ACTIVE_CONTEXT.with(|ctx| {
                ctx.borrow_mut()
                    .entries
                    .push(AsyncContextEntry { handle, stores });
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
