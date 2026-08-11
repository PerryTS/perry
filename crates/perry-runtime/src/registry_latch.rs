//! Monotone "has this feature ever been used?" latches for side-table probes.
//!
//! The runtime answers "is this value special?" for a large number of exotic
//! kinds — typed array, `Buffer`, `SharedArrayBuffer` backing, `Symbol`,
//! `DataView`, `ArrayBuffer`, `Map`, `Set` — by consulting an address-keyed
//! side table. Those probes sit on *generic* paths (property get/set, element
//! access, `typeof`, coercion, `JSON.stringify`, console formatting, GC header
//! reads), so a program pays for every kind it does not use, on every value it
//! touches. Each probe costs at minimum a thread-local resolution (on Darwin
//! that is a real call through `_tlv_get_addr` — there is no local-exec TLS)
//! plus a `RefCell` borrow and a hash, and for the process-global tables a
//! mutex acquisition.
//!
//! Symbolicated profiles of two unrelated realistic programs measured **13% of
//! total runtime** in exactly these probes, for features neither program used:
//! an async service pipeline spent 2.45% in `lookup_typed_array_kind`, 2.40% in
//! `is_registered_buffer`, 1.22% in `is_shared_sab` and 0.71% in
//! `is_registered_symbol` while allocating no typed array, no `Buffer`, no
//! `SharedArrayBuffer` and no `Symbol`; a tree-walking interpreter showed the
//! same two leaders independently.
//!
//! [`RegistryLatch`] removes that tax. It is a process-global `AtomicBool` that
//! starts `false` and is armed by the *registration* site. A probe checks it
//! first and answers "no" from a single atomic load when the feature has never
//! been used. #7474 established the pattern for `map`/`set`; this type
//! generalises it so the remaining probes get it without copy-paste.
//!
//! # Why monotone
//!
//! The latch has no `disarm`, deliberately — there is no counter to get wrong
//! and no ordering hazard between an unregister and a concurrent probe. Once
//! armed the process pays the ordinary slow path forever, which is merely
//! slower, never wrong. That asymmetry is the whole safety argument: the only
//! *incorrect* observation this design can produce is `idle` while a table is
//! non-empty. `armed` while every table is empty is free of consequence.
//!
//! # The ordering rule (binding)
//!
//! **`arm()` must be called BEFORE the registry mutation it advertises, in the
//! registering thread's program order.** Arming *after* the insert opens a
//! window in which the feature is live and reachable but the latch still reads
//! idle, so a concurrent probe takes the fast path and answers `false` for an
//! address that is genuinely registered. That is not hypothetical: the sibling
//! latch `buffer::header::EXTERNAL_BUFFERS_NONEMPTY` carries an inline comment
//! for precisely this reason, and `js_buffer_register_external` latches first.
//!
//! With the arm placed first, the argument for a reader on *another* thread is:
//! a thread can only probe an address it holds, and every route by which an
//! address reaches a different thread in this runtime passes through a
//! synchronising edge (the `SerializedValue` deep-copy queue and the
//! `PENDING_THREAD_RESULTS` drain are both mutex/channel mediated). The arm
//! precedes the registration, which precedes the hand-off, so the arm is in the
//! reader's happens-before past and the reader must observe it. The
//! thread-local tables (`BUFFER_REGISTRY`, `TYPED_ARRAY_REGISTRY`,
//! `UINT8ARRAY_FROM_CTOR`) need even less: only the arming thread can find
//! their entries at all, and a thread always observes its own prior store.
//!
//! `Acquire`/`Release` is therefore stronger than today's routes require —
//! `Relaxed` would be sound given the hand-off edges above. It costs one
//! instruction and removes the need to re-audit this file the next time someone
//! publishes a heap address through a lock-free path, so the stronger ordering
//! is what ships.

use std::sync::atomic::{AtomicBool, Ordering};

/// A one-way "this feature has been registered at least once" flag.
///
/// See the module docs for the ordering rule: arm before you publish.
#[derive(Debug, Default)]
pub struct RegistryLatch {
    armed: AtomicBool,
}

impl RegistryLatch {
    /// A latch that has never been armed.
    pub const fn new() -> Self {
        Self {
            armed: AtomicBool::new(false),
        }
    }

    /// True while nothing has ever been registered, so an address-keyed probe
    /// over the guarded table can answer "not found" without touching it.
    ///
    /// This is the hot side: one relaxed-cost atomic load in place of a
    /// thread-local resolution plus a hash probe (plus, for the global tables,
    /// a mutex acquisition).
    #[inline(always)]
    pub fn is_idle(&self) -> bool {
        !self.armed.load(Ordering::Acquire)
    }

    /// True once anything has ever been registered. Never goes back to false.
    #[inline(always)]
    pub fn is_armed(&self) -> bool {
        self.armed.load(Ordering::Acquire)
    }

    /// Publish "this feature is now in use".
    ///
    /// MUST run **before** the guarded table is mutated — see the module docs.
    ///
    /// The load-then-store keeps a hot registration loop from re-dirtying a
    /// shared cache line on every allocation once the latch is already armed;
    /// skipping the store is sound because some earlier `Release` store already
    /// established the value, and the latch never travels back to `false`.
    #[inline]
    pub fn arm(&self) {
        if !self.armed.load(Ordering::Relaxed) {
            self.armed.store(true, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_idle_and_arms_once() {
        let latch = RegistryLatch::new();
        assert!(latch.is_idle());
        assert!(!latch.is_armed());
        latch.arm();
        assert!(!latch.is_idle());
        assert!(latch.is_armed());
        // Monotone: re-arming is a no-op, and there is deliberately no way back.
        latch.arm();
        assert!(latch.is_armed());
    }

    #[test]
    fn arm_is_visible_to_another_thread() {
        static LATCH: RegistryLatch = RegistryLatch::new();
        assert!(LATCH.is_idle());
        std::thread::spawn(|| LATCH.arm()).join().unwrap();
        assert!(LATCH.is_armed());
    }
}
