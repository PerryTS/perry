//! `guard_cleared_global!` — a process-global side table that the GC test
//! guards CLEAR wholesale, declared so that the clear cannot reach another
//! test's data.
//!
//! # The defect this exists to make structurally impossible (#7672)
//!
//! `gc::tests::support::reset_copying_nursery_runtime_test_state()` runs from
//! `GcTestIsolationGuard` and `CopyingNurseryTestGuard`, and calls ~20
//! `test_clear_*` helpers so the GC test that constructed the guard sees
//! exactly the roots it installs. Those helpers `clear()` PROCESS-global
//! tables, from whatever libtest thread happens to be running the guard.
//!
//! The guards serialize against *each other* (`global_side_table_test_lock`),
//! and against the handful of tests that remember to take that lock. Nothing
//! requires a *reader* to take it — the defence is opt-in and the opt-in is
//! invisible at the read site. So a test on thread T that writes an entry and
//! reads it back a few statements later has its entry deleted, mid-test, by a
//! guard on thread U, and observes a silently wrong value.
//!
//! Three flakes in two days came from exactly this, each diagnosed from the
//! wrong VALUE rather than the timing:
//!
//! | fixed in | table | presented as |
//! |---|---|---|
//! | #7665 | `opt_report`'s row sink | `rows.len() == 2` failing at **3** |
//! | #7665 | `ext_registry`'s `USED_PROVIDERS` | "empty" failing with `ioredis` present |
//! | #7671 | `closure`'s `CLOSURE_PROPS` | a static method read back as `TAG_UNDEFINED` |
//!
//! # Why the fix is per-thread storage and not a lock
//!
//! The damage window is "between this test's write and this test's read", and
//! only the test knows that span. An accessor that takes a shared lock for the
//! duration of one call does not cover it; a lock that covers it has to be
//! taken by the test, which is the opt-in the class is made of. Blanket-locking
//! the ~180 tests that read these tables serializes a large part of the suite
//! for a hazard whose incidence nobody can bound.
//!
//! So the storage moves instead. In a **test build** each thread gets its own
//! instance of the table, which is exactly the isolation the clear was reaching
//! for: `reset_copying_nursery_runtime_test_state()` on thread U empties U's
//! instance, which already contains only U's entries, and thread T's test is
//! structurally out of reach. libtest runs one thread per test, so "per thread"
//! and "per test" coincide.
//!
//! In a **non-test build** the macro expands to the plain `static` it replaced,
//! byte for byte. `test_clear_*` is `#[cfg(test)]` and never runs in a shipped
//! runtime, so the whole hazard — and the whole fix — is confined to tests.
//!
//! Call sites do not change: [`PerThread`] derefs to the table, so
//! `SYMBOL_REGISTRY.lock()` and `CLASS_PROTOTYPE_OBJECTS.read()` keep working
//! as written.
//!
//! # What holds the line
//!
//! * `scripts/global_sink_isolation.py` (runs in `lint`) derives the clear list
//!   from `reset_copying_nursery_runtime_test_state`'s own source, resolves the
//!   storage behind each helper, and FAILS on a bare `static` that is neither
//!   thread-local nor declared here. A new sink cannot be added quietly, and a
//!   new *reader* never has to remember anything.
//! * `gc::tests::global_sink_isolation` plants the #7671 shape — write on this
//!   thread, run the guards' clear on another, read back — for each converted
//!   table, and proves the harness still catches an unconverted one via a
//!   deliberately bare canary. A green run means the detector works, not that
//!   nothing was tried.

/// Per-thread instance of a process-global side table, for test builds.
///
/// Constructed only by [`guard_cleared_global!`]. Derefs to `T`, so every call
/// site that used the `static` directly is unchanged.
///
/// The instance is leaked (`Box::leak`) rather than dropped at thread exit:
/// `Deref::deref` must hand out a reference that outlives the borrow of the
/// `static`, and a thread-exit drop would dangle it. The cost is bounded by
/// (threads that touched the table) x (empty table size); a libtest thread that
/// never touches a table never allocates one.
#[cfg(test)]
pub struct PerThread<T: Send + Sync + 'static> {
    init: fn() -> T,
}

#[cfg(test)]
impl<T: Send + Sync + 'static> PerThread<T> {
    pub const fn new(init: fn() -> T) -> Self {
        Self { init }
    }

    fn instance(&self) -> &'static T {
        let key = self as *const Self as usize;
        // A thread-local map, not a global one: a global would reintroduce a
        // process-wide lock on a path taken by every side-table access in the
        // test build.
        let cached = SLOTS.try_with(|slots| slots.borrow().get(&key).copied());
        if let Ok(Some(addr)) = cached {
            // SAFETY: `addr` was produced below from a `Box::leak` of `T` on
            // this thread and is never removed, so it is live for the process.
            return unsafe { &*(addr as *const T) };
        }
        let leaked: &'static T = Box::leak(Box::new((self.init)()));
        // `try_with` fails only while this thread's TLS is being destroyed. A
        // table touched from a TLS destructor gets a fresh instance rather than
        // a panic; it is being torn down either way.
        let _ = SLOTS.try_with(|slots| {
            slots
                .borrow_mut()
                .insert(key, leaked as *const T as usize);
        });
        leaked
    }
}

#[cfg(test)]
impl<T: Send + Sync + 'static> std::ops::Deref for PerThread<T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.instance()
    }
}

// SAFETY: every thread only ever observes the instance it created, and the
// instances are leaked, so no `T` is shared or dropped across threads by this
// type. `T: Send + Sync` is required anyway because the non-test expansion is a
// plain `static`.
#[cfg(test)]
unsafe impl<T: Send + Sync + 'static> Sync for PerThread<T> {}

#[cfg(test)]
thread_local! {
    /// `&PerThread<T>` address -> leaked `&'static T` address, for this thread.
    static SLOTS: std::cell::RefCell<std::collections::HashMap<usize, usize>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Declare a process-global side table that the GC test guards clear.
///
/// Expands to the plain `static` outside a test build, and to a
/// [`PerThread`] instance inside one. See the module docs for why.
///
/// ```ignore
/// guard_cleared_global! {
///     /// Doc comments and attributes pass through.
///     static SYMBOL_REGISTRY: Mutex<Option<HashMap<String, usize>>> = Mutex::new(None);
/// }
/// ```
macro_rules! guard_cleared_global {
    ($(
        $(#[$attr:meta])*
        $vis:vis static $name:ident : $ty:ty = $init:expr;
    )+) => {$(
        #[cfg(not(test))]
        $(#[$attr])*
        $vis static $name: $ty = $init;

        #[cfg(test)]
        $(#[$attr])*
        $vis static $name: $crate::guard_cleared_global::PerThread<$ty> =
            $crate::guard_cleared_global::PerThread::new(|| $init);
    )+};
}
