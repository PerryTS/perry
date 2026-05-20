//! AsyncLocalStorage context propagation support.
//!
//! This module owns the thread-local execution context used by
//! `node:async_hooks` AsyncLocalStorage. The stdlib module mutates the active
//! context; async schedulers snapshot it when work is queued and restore it
//! while the callback runs.

use std::cell::RefCell;

#[derive(Clone, Default)]
pub struct AsyncContextSnapshot {
    entries: Vec<AsyncContextEntry>,
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

pub fn enter_with(handle: i64, store: f64) {
    push_store(handle, store);
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

pub fn scan_active_context_roots(mark: &mut dyn FnMut(f64)) {
    ACTIVE_CONTEXT.with(|ctx| {
        let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
        scan_snapshot_roots_mut(&mut ctx.borrow_mut(), &mut visitor);
    });
}

pub fn scan_active_context_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    ACTIVE_CONTEXT.with(|ctx| {
        scan_snapshot_roots_mut(&mut ctx.borrow_mut(), visitor);
    });
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
