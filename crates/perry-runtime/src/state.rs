//! #6759 Phase A: explicit per-thread runtime state (perry's `Isolate`).
//!
//! Historically every piece of object-model metadata lived in its own
//! `thread_local!` side table, so answering "what are property X's
//! attributes on this receiver" cost one TLS resolution (`_tlv_get_addr`
//! plus `LocalKey::with`'s lazy-init/destructor bookkeeping) *per table
//! probed*. This module concentrates the hot tables into one heap-allocated
//! [`RuntimeState`] reached through a single const-initialized TLS pointer:
//! the fast path of [`state`] is one TLS address computation and one load —
//! no init flag, no destructor registration — and a hot function that
//! probes several tables can fetch the state once into a local and reuse
//! it.
//!
//! Isolation semantics are unchanged: each OS thread that touches the
//! runtime (the main JS thread, every `perry/thread` worker, and any tokio
//! callback that reaches an object helper) lazily allocates its own
//! `RuntimeState` on first use, exactly as each `thread_local!` table used
//! to lazily initialize per thread. The state is freed at thread exit via
//! [`StateOwner`]'s TLS destructor, mirroring the drop the old per-table
//! TLS values received.
//!
//! Borrow discipline is also unchanged for now (fields keep their
//! `RefCell`/`Cell`/`UnsafeCell` wrappers); relaxing it is explicitly a
//! later step in #6759.

use std::cell::{Cell, RefCell};

/// The per-thread runtime state. Grows one field group at a time as tables
/// migrate out of module-level `thread_local!`s (#6759 Phase A is
/// explicitly incremental); each group struct lives next to the code that
/// owns it so the table types stay private to their module.
pub(crate) struct RuntimeState {
    /// Property/accessor descriptor tables + their fast-path gates
    /// (previously `object::descriptor_state`'s four `thread_local!`s).
    pub(crate) descriptors: crate::object::DescriptorTables,
    /// Object field storage side tables: overflow fields, keys-index
    /// sidecar, shape and transition caches (previously five
    /// `thread_local!`s in `object::mod`).
    pub(crate) object_hot: crate::object::ObjectHotTables,
    /// Property-lookup inline caches: the direct-mapped field cache and
    /// the wide-object key index (previously `thread_local!`s in
    /// `object::field_get_set`).
    pub(crate) field_lookup: crate::object::FieldLookupCaches,
}

impl RuntimeState {
    fn new_boxed() -> Box<Self> {
        Box::new(RuntimeState {
            descriptors: crate::object::DescriptorTables::new(),
            object_hot: crate::object::ObjectHotTables::new(),
            field_lookup: crate::object::FieldLookupCaches::new(),
        })
    }
}

thread_local! {
    /// Fast-path pointer to this thread's state. `Cell<*mut _>` has no drop
    /// glue, so this TLS slot never registers a destructor — `with` on it
    /// compiles down to the raw TLS address computation + load, and it
    /// remains accessible from other TLS destructors during thread
    /// teardown.
    static STATE_PTR: Cell<*mut RuntimeState> = const { Cell::new(std::ptr::null_mut()) };
    /// Owns the allocation behind [`STATE_PTR`]; its destructor frees the
    /// state at thread exit (and nulls the fast-path pointer first, so a
    /// late access from another TLS destructor re-initializes instead of
    /// dereferencing a freed pointer).
    static STATE_OWNER: RefCell<Option<StateOwner>> = const { RefCell::new(None) };
}

struct StateOwner(*mut RuntimeState);

impl Drop for StateOwner {
    fn drop(&mut self) {
        STATE_PTR.with(|c| c.set(std::ptr::null_mut()));
        // Safety: `self.0` came from `Box::into_raw` in `init_state` and is
        // only freed here; nulling STATE_PTR above keeps any later `state()`
        // call on this thread from handing out the dangling pointer.
        unsafe { drop(Box::from_raw(self.0)) };
    }
}

/// Fetch this thread's [`RuntimeState`], allocating it on first use.
///
/// Hot paths that touch several tables should call this once and keep the
/// reference in a local. The returned `&'static` is sound for runtime code
/// because every caller runs on the thread that owns the state and cannot
/// outlive it: the state lives until thread exit, and runtime entry points
/// never park a reference across threads.
#[inline]
pub(crate) fn state() -> &'static RuntimeState {
    let p = STATE_PTR.with(|c| c.get());
    if p.is_null() {
        init_state()
    } else {
        unsafe { &*p }
    }
}

#[cold]
#[inline(never)]
fn init_state() -> &'static RuntimeState {
    let raw = Box::into_raw(RuntimeState::new_boxed());
    // Register the owner so the state is freed at thread exit. During thread
    // teardown STATE_OWNER may already be destroyed (`try_with` fails); the
    // state then leaks for the remainder of teardown, which is the safe
    // choice — only other TLS destructors can still reach it.
    let _ = STATE_OWNER.try_with(|o| *o.borrow_mut() = Some(StateOwner(raw)));
    STATE_PTR.with(|c| c.set(raw));
    unsafe { &*raw }
}
