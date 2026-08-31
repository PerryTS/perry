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

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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

/// A monotone "smallest and largest address ever registered" window.
///
/// [`RegistryLatch`] answers "has this feature EVER been used?". That question
/// stops discriminating the moment a program registers its first entry — and
/// for the two hottest probes in the runtime it stops discriminating almost
/// immediately: a `claude-code --help` run registers **10** buffers and **42**
/// typed arrays, then probes `is_registered_buffer` 4.65 M times and
/// `lookup_typed_array_kind` 3.57 M times. Measured on that binary, the buffer
/// probe answered `true` **4** times out of 4,651,086. The latch was armed for
/// every one of those calls, so all 4.65 M paid the out-of-line call, a
/// thread-local resolution, a `RefCell` borrow and a hash to say "no".
///
/// This window is the same monotone idea applied to the address instead of to
/// the fact of registration: every registration widens `[lo, hi]` *before* it
/// publishes, so an address outside the window cannot be in any table the
/// window covers. Rejecting is therefore sound; accepting merely falls through
/// to the exact lookup that was already there.
///
/// It is strictly stronger than a latch — an unregistered process has the empty
/// window `[usize::MAX, 0]`, which contains nothing — and it costs two adjacent
/// static loads and two compares, which inline into the probe's call sites
/// instead of being paid behind a call.
///
/// # The ordering rule (binding, and identical to [`RegistryLatch`]'s)
///
/// **[`admit`](Self::admit) must run BEFORE the registry mutation it
/// advertises.** Widening after the insert opens a window in which an address
/// is registered but outside the published range, so a concurrent probe would
/// answer `false` for an address that is genuinely registered.
///
/// `lo` and `hi` are separate atomics, so a racing reader can observe a mix of
/// old and new values. That is harmless: each moves in one direction only, so
/// for any address `a` this thread has admitted, this thread's own subsequent
/// loads must see `lo <= a` and `hi >= a` (program order plus monotonicity).
/// Cross-thread visibility rests on the same hand-off edges the latch documents
/// above.
#[derive(Debug)]
pub struct RegistryAddrWindow {
    lo: AtomicUsize,
    hi: AtomicUsize,
}

impl Default for RegistryAddrWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl RegistryAddrWindow {
    /// An empty window: contains no address at all.
    pub const fn new() -> Self {
        Self {
            lo: AtomicUsize::new(usize::MAX),
            hi: AtomicUsize::new(0),
        }
    }

    /// `false` ⟹ `addr` is definitively absent from every table this window
    /// covers, so the caller can answer "not found" without touching one.
    ///
    /// This is the hot side. It is deliberately `inline(always)`: the whole
    /// point is that the common negative answer costs a couple of loads at the
    /// call site rather than a call into the registry probe.
    #[inline(always)]
    pub fn may_contain(&self, addr: usize) -> bool {
        addr >= self.lo.load(Ordering::Acquire) && addr <= self.hi.load(Ordering::Acquire)
    }

    /// Widen the window to include `addr`.
    ///
    /// MUST run **before** the guarded table is mutated — see the type docs.
    ///
    /// `fetch_min`/`fetch_max`, not load-then-store: a plain store is a
    /// read-modify-write with a hole in it, and two threads registering at
    /// once can both read the old bound and let the *narrower* of the two
    /// stores land last — dropping the other thread's address out of the
    /// window while its entry is live in that thread's registry. That is a
    /// false negative for a genuinely registered address, i.e. exactly the one
    /// failure mode this type must not have. The relaxed pre-check keeps the
    /// atomic RMW off the path a registration loop takes once the window
    /// already covers the address; skipping is sound because the bound only
    /// ever moves outward, so an observed `lo <= addr` can never later become
    /// `lo > addr`.
    #[inline]
    pub fn admit(&self, addr: usize) {
        if self.lo.load(Ordering::Relaxed) > addr {
            self.lo.fetch_min(addr, Ordering::Release);
        }
        if self.hi.load(Ordering::Relaxed) < addr {
            self.hi.fetch_max(addr, Ordering::Release);
        }
    }

    /// Test hook: the current `[lo, hi]` pair, or `None` while empty.
    #[cfg(test)]
    pub(crate) fn bounds_for_tests(&self) -> Option<(usize, usize)> {
        let lo = self.lo.load(Ordering::Acquire);
        let hi = self.hi.load(Ordering::Acquire);
        (lo <= hi).then_some((lo, hi))
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

    #[test]
    fn empty_window_contains_nothing() {
        let w = RegistryAddrWindow::new();
        assert_eq!(w.bounds_for_tests(), None);
        for addr in [0usize, 1, 0x1000, usize::MAX / 2, usize::MAX] {
            assert!(
                !w.may_contain(addr),
                "an empty window must reject {addr:#x} — it stands in for an idle latch"
            );
        }
    }

    #[test]
    fn window_only_ever_widens_and_never_rejects_an_admitted_address() {
        let w = RegistryAddrWindow::new();
        w.admit(0x3000);
        assert_eq!(w.bounds_for_tests(), Some((0x3000, 0x3000)));
        assert!(w.may_contain(0x3000));
        assert!(!w.may_contain(0x2fff));
        assert!(!w.may_contain(0x3001));

        w.admit(0x9000);
        assert_eq!(w.bounds_for_tests(), Some((0x3000, 0x9000)));
        // Both admitted addresses stay inside, and so does everything between.
        assert!(w.may_contain(0x3000));
        assert!(w.may_contain(0x6000));
        assert!(w.may_contain(0x9000));
        assert!(!w.may_contain(0x2fff));
        assert!(!w.may_contain(0x9001));

        // Re-admitting an interior address must not narrow anything.
        w.admit(0x6000);
        assert_eq!(w.bounds_for_tests(), Some((0x3000, 0x9000)));
    }

    /// Concurrent registration must not lose an address. With a load-then-store
    /// `admit` the two threads' stores race and the narrower bound can land
    /// last, evicting the other thread's live registration from the window —
    /// a false negative, which is a misclassification rather than a slowdown.
    /// `fetch_min`/`fetch_max` make that impossible, so this passes
    /// deterministically here and fails with high probability on the racy form.
    #[test]
    fn concurrent_admits_never_drop_an_address() {
        const THREADS: usize = 8;
        const PER_THREAD: usize = 512;
        static WINDOW: RegistryAddrWindow = RegistryAddrWindow::new();
        let handles: Vec<_> = (0..THREADS)
            .map(|t| {
                std::thread::spawn(move || {
                    // Interleave the ranges so every thread admits both very
                    // low and very high addresses, maximising the number of
                    // genuine bound updates that can race.
                    for i in 0..PER_THREAD {
                        WINDOW.admit(0x1_0000 + i * THREADS + t);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        for t in 0..THREADS {
            for i in 0..PER_THREAD {
                let addr = 0x1_0000 + i * THREADS + t;
                assert!(
                    WINDOW.may_contain(addr),
                    "{addr:#x} was admitted but the window lost it: {:?}",
                    WINDOW.bounds_for_tests()
                );
            }
        }
    }

    #[test]
    fn window_admit_is_visible_to_another_thread() {
        static WINDOW: RegistryAddrWindow = RegistryAddrWindow::new();
        assert!(!WINDOW.may_contain(0x4000));
        std::thread::spawn(|| WINDOW.admit(0x4000)).join().unwrap();
        assert!(WINDOW.may_contain(0x4000));
    }
}
