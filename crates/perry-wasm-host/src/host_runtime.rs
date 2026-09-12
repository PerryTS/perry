//! Thread-local shared wasmi engine/store access.

use std::cell::{Cell, UnsafeCell};
use wasmi::{Engine, Store};

/// All WebAssembly objects in one JavaScript agent share an engine and store.
/// The borrow flag rejects JavaScript re-entry before a second mutable
/// reference to that store can be created.
pub(super) struct HostRuntime {
    pub(super) engine: Engine,
    pub(super) store: Store<()>,
}

thread_local! {
    static HOST_RUNTIME: UnsafeCell<HostRuntime> = UnsafeCell::new({
        let engine = Engine::default();
        let store = Store::new(&engine, ());
        HostRuntime { engine, store }
    });
    static HOST_RUNTIME_BORROWED: Cell<bool> = const { Cell::new(false) };
}

struct HostRuntimeBorrowGuard;

impl HostRuntimeBorrowGuard {
    fn enter() -> Option<Self> {
        HOST_RUNTIME_BORROWED.with(|borrowed| {
            if borrowed.replace(true) {
                None
            } else {
                Some(Self)
            }
        })
    }
}

impl Drop for HostRuntimeBorrowGuard {
    fn drop(&mut self) {
        HOST_RUNTIME_BORROWED.with(|borrowed| borrowed.set(false));
    }
}

pub(super) fn with_host_runtime<R>(f: impl FnOnce(&mut HostRuntime) -> R) -> Option<R> {
    let _guard = HostRuntimeBorrowGuard::enter()?;
    Some(HOST_RUNTIME.with(|runtime| unsafe { f(&mut *runtime.get()) }))
}
