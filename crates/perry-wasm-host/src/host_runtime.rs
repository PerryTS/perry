//! Thread-local shared wasmi engine/store access.
//!
//! All WebAssembly objects in one JavaScript agent share an engine and store.
//! A JavaScript import can legitimately re-enter WebAssembly while an outer
//! call holds the store: Emscripten's `invoke_*`/`dynCall` trampolines call a
//! table function, and imports call exports such as `_malloc`. wasmi hands the
//! import its `Caller`, which is the only live mutable view of the store for
//! that call's extent, so nested host access is routed through the innermost
//! active `Caller` instead of borrowing the thread-local store a second time.

use std::cell::{Cell, RefCell, UnsafeCell};
use wasmi::{AsContextMut, Caller, Engine, Store, StoreContextMut};

/// The store context for one host operation: the thread-local store at the
/// outermost level, or the active import callback's `Caller` when nested.
pub(super) struct HostRuntime<'a> {
    pub(super) engine: &'a Engine,
    pub(super) store: StoreContextMut<'a, ()>,
}

struct SharedRuntime {
    engine: Engine,
    store: Store<()>,
}

/// One import callback's `Caller`, lifetime-erased. It is valid exactly while
/// its `ImportCallbackScope` is alive; `busy` marks a host operation currently
/// holding a mutable view through it.
struct CallerLevel {
    caller: *mut (),
    busy: bool,
}

thread_local! {
    static HOST_RUNTIME: UnsafeCell<SharedRuntime> = UnsafeCell::new({
        let engine = Engine::default();
        let store = Store::new(&engine, ());
        SharedRuntime { engine, store }
    });
    static ROOT_BUSY: Cell<bool> = const { Cell::new(false) };
    static CALLER_LEVELS: RefCell<Vec<CallerLevel>> = const { RefCell::new(Vec::new()) };
}

struct RootBusyGuard;

impl Drop for RootBusyGuard {
    fn drop(&mut self) {
        ROOT_BUSY.with(|busy| busy.set(false));
    }
}

struct CallerBusyGuard(usize);

impl Drop for CallerBusyGuard {
    fn drop(&mut self) {
        CALLER_LEVELS.with(|levels| {
            if let Some(level) = levels.borrow_mut().get_mut(self.0) {
                level.busy = false;
            }
        });
    }
}

/// Run `f` with a mutable view of this agent's store. Returns `None` only for
/// a genuine aliasing attempt: re-entry that is not inside an import callback
/// of the operation already holding the store.
pub(super) fn with_host_runtime<R>(f: impl FnOnce(&mut HostRuntime<'_>) -> R) -> Option<R> {
    HOST_RUNTIME.with(|shared| {
        let shared = shared.get();
        // Field projections only: the engine is shared, and the store is
        // reached either here (outermost) or through a Caller (nested).
        let engine = unsafe { &*std::ptr::addr_of!((*shared).engine) };
        let nested = CALLER_LEVELS.with(|levels| {
            let mut levels = levels.borrow_mut();
            let index = levels.len().checked_sub(1)?;
            let level = &mut levels[index];
            Some((!level.busy).then(|| {
                level.busy = true;
                (index, level.caller)
            }))
        });
        match nested {
            Some(None) => None,
            Some(Some((index, caller))) => {
                let _busy = CallerBusyGuard(index);
                let caller = unsafe { &mut *(caller as *mut Caller<'_, ()>) };
                let mut runtime = HostRuntime {
                    engine,
                    store: caller.as_context_mut(),
                };
                Some(f(&mut runtime))
            }
            None => {
                if ROOT_BUSY.with(|busy| busy.replace(true)) {
                    return None;
                }
                let _busy = RootBusyGuard;
                let store = unsafe { &mut *std::ptr::addr_of_mut!((*shared).store) };
                let mut runtime = HostRuntime {
                    engine,
                    store: store.as_context_mut(),
                };
                Some(f(&mut runtime))
            }
        }
    })
}

/// The agent's store address, recorded by instances for their direct
/// accessors. Obtained without creating a reference to the store.
pub(super) fn host_store_ptr() -> *mut Store<()> {
    HOST_RUNTIME.with(|shared| unsafe { std::ptr::addr_of_mut!((*shared.get()).store) })
}

/// Publishes an import callback's `Caller` as the innermost store context for
/// the duration of the JavaScript callback. The trampoline must not use
/// `caller` itself while the scope is alive.
pub(super) struct ImportCallbackScope;

impl ImportCallbackScope {
    pub(super) fn enter(caller: &mut Caller<'_, ()>) -> Self {
        let caller = caller as *mut Caller<'_, ()> as *mut ();
        CALLER_LEVELS.with(|levels| {
            levels.borrow_mut().push(CallerLevel {
                caller,
                busy: false,
            })
        });
        Self
    }
}

impl Drop for ImportCallbackScope {
    fn drop(&mut self) {
        CALLER_LEVELS.with(|levels| {
            levels.borrow_mut().pop();
        });
    }
}
