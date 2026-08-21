//! Path-module registry: the once-only, waitable initialization state for
//! AOT-compiled modules that a runtime `require(absolutePath)` can load.
//!
//! Split out of `module_require.rs` for the 2000-line file cap; the FFI entry
//! points that drive it stay with the rest of the `require` surface.

/// State of one AOT-compiled module that can be loaded by runtime path.
///
/// `Initializing` carries the owner thread so a CommonJS cycle on that same
/// thread can observe the wrapper's partial `exports` object. The registry's
/// loader-wide logical owner prevents a second thread from claiming another
/// key while generated init code is active, so concurrent A <-> B loads cannot
/// form a wait cycle. Other callers wait for the final value or failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PathModuleStatus {
    Registered,
    Initializing(std::thread::ThreadId),
    Initialized,
    /// Initializers are not retried. Every later caller receives the exact
    /// same thrown JS value until process teardown.
    Failed(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PathModuleRequireError {
    Initializer(u64),
    /// Defensive fail-closed result for a state that would otherwise make the
    /// logical loader owner wait on a foreign initializer.
    OwnershipConflict,
}

#[derive(Debug)]
struct PathModuleEntry {
    init_addr: Option<usize>,
    /// `Option` is the presence bit: `Some(TAG_UNDEFINED)` is a real,
    /// initialized CommonJS export and must not be confused with a miss.
    exports: Option<u64>,
    status: PathModuleStatus,
    /// A generated initializer claimed through `require_with`. Wrapper
    /// partial/final publication also runs for eagerly initialized modules, so
    /// `init_addr` alone cannot distinguish the two completion boundaries.
    active_claim: Option<std::thread::ThreadId>,
}

#[derive(Default)]
struct PathModuleState {
    entries: std::collections::HashMap<String, PathModuleEntry>,
    /// Logical (not mutex) ownership of the lazy loader. While one thread is
    /// running generated init code, only that thread may claim another path.
    /// This composes concurrency with A -> B -> A cycles without ever holding
    /// the registry mutex across generated code.
    loader_owner: Option<std::thread::ThreadId>,
    loader_depth: usize,
    /// Every entry currently in `Initializing` belongs to this thread.
    /// Counting avoids scanning thousands of registered Next modules on each
    /// cold claim.
    initializing_owner: Option<std::thread::ThreadId>,
    initializing_count: usize,
    /// Eager CJS wrappers that have published partial exports, in nesting
    /// order. The native module-init exception boundary fails only the current
    /// wrapper, so an inner error that user code catches does not poison its
    /// caller.
    eager_initializers: Vec<(std::thread::ThreadId, String, Option<u64>)>,
    module_boundaries: Vec<(std::thread::ThreadId, u64)>,
    next_module_boundary: u64,
}

impl PathModuleState {
    fn begin_initializing(&mut self, owner: std::thread::ThreadId) -> bool {
        match self.initializing_owner {
            Some(existing) if existing != owner => false,
            Some(_) => {
                self.initializing_count += 1;
                true
            }
            None => {
                self.initializing_owner = Some(owner);
                self.initializing_count = 1;
                true
            }
        }
    }

    fn finish_initializing(&mut self, owner: std::thread::ThreadId) {
        debug_assert_eq!(self.initializing_owner, Some(owner));
        debug_assert!(self.initializing_count > 0);
        self.initializing_count -= 1;
        if self.initializing_count == 0 {
            self.initializing_owner = None;
        }
    }

    fn finish_eager(&mut self, owner: std::thread::ThreadId, key: &str) {
        let index = self
            .eager_initializers
            .iter()
            .rposition(|(candidate_owner, candidate_key, _)| {
                *candidate_owner == owner && candidate_key == key
            })
            .expect("completed eager path module was missing from the init stack");
        debug_assert_eq!(index + 1, self.eager_initializers.len());
        self.eager_initializers.remove(index);
    }

    fn current_module_boundary(&self, owner: std::thread::ThreadId) -> Option<u64> {
        self.module_boundaries
            .iter()
            .rfind(|(candidate_owner, _)| *candidate_owner == owner)
            .map(|(_, boundary)| *boundary)
    }
}

/// Provider-visible path-module registry. App-only dylibs call these runtime
/// symbols through their undefined ABI references, so the state lives in the
/// separately loaded runtime provider rather than being duplicated per app.
/// One mutex protects logical loader ownership and export publication
/// atomically; it is always released before generated code runs.
pub(super) struct PathModuleRegistry {
    state: std::sync::Mutex<PathModuleState>,
    ready: std::sync::Condvar,
}

impl Default for PathModuleRegistry {
    fn default() -> Self {
        Self {
            state: std::sync::Mutex::new(PathModuleState::default()),
            ready: std::sync::Condvar::new(),
        }
    }
}

impl PathModuleRegistry {
    fn lock(&self) -> std::sync::MutexGuard<'_, PathModuleState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Register one canonical path -> initializer mapping. The same mapping is
    /// idempotent. A second address for the same canonical file is rejected so
    /// an alias can never create a second logical module initialization.
    pub(super) fn register_init(&self, key: String, init_addr: usize) -> bool {
        // The address is a property of the PROGRAM, not of any heap: codegen
        // emits one `<prefix>__init` per canonical path and registers it once,
        // on whichever thread runs module init first. Every later heap needs
        // that same address to be able to initialize the module for itself, so
        // it lives in a process-global map. It is a code pointer, never a JS
        // value, so no collector has any interest in it.
        if !register_init_addr(&key, init_addr) {
            return false;
        }
        let mut state = self.lock();
        let entry = state.entries.entry(key).or_insert_with(|| PathModuleEntry {
            init_addr: None,
            exports: None,
            status: PathModuleStatus::Registered,
            active_claim: None,
        });
        entry.init_addr = Some(init_addr);
        true
    }

    /// Adopt the program-wide initializer for `key` into THIS heap's table.
    /// A heap that has never required the path has no entry for it, but the
    /// initializer exists process-wide; without this a second heap silently
    /// resolves the module to an empty `module.exports`.
    fn adopt_registered_init(&self, state: &mut PathModuleState, key: &str) -> bool {
        if state.entries.contains_key(key) {
            return true;
        }
        let Some(init_addr) = lookup_init_addr(key) else {
            return false;
        };
        state.entries.insert(
            key.to_string(),
            PathModuleEntry {
                init_addr: Some(init_addr),
                exports: None,
                status: PathModuleStatus::Registered,
                active_claim: None,
            },
        );
        true
    }

    /// Publish the initial CommonJS `exports` object before the wrapper body.
    /// Only same-thread recursive loads may observe it; unrelated waiters stay
    /// parked while the status is `Initializing`.
    pub(super) fn register_partial_exports(&self, key: String, exports: u64) -> bool {
        let current = std::thread::current().id();
        let stored = {
            let mut state = self.lock();
            if state.loader_owner.is_some_and(|owner| owner != current)
                || state
                    .initializing_owner
                    .is_some_and(|owner| owner != current)
            {
                // Generated code on a second thread must not create a foreign
                // `Initializing` entry while the logical loader is owned. If
                // it did, the owner could wait for this entry while this
                // thread waits for the owner's module. The FFI caller turns
                // this rejection into a JS exception.
                return false;
            }
            let key_for_stack = key.clone();
            let (began, eager) = {
                let entry = state.entries.entry(key).or_insert_with(|| PathModuleEntry {
                    init_addr: None,
                    exports: None,
                    status: PathModuleStatus::Registered,
                    active_claim: None,
                });
                if matches!(entry.status, PathModuleStatus::Failed(_))
                    || matches!(entry.status, PathModuleStatus::Initializing(owner) if owner != current)
                {
                    return false;
                }
                entry.exports = Some(exports);
                if entry.status == PathModuleStatus::Registered {
                    entry.status = PathModuleStatus::Initializing(current);
                    (true, entry.active_claim.is_none())
                } else {
                    (false, false)
                }
            };
            if began {
                if !state.begin_initializing(current) {
                    return false;
                }
                if eager {
                    let boundary = state.current_module_boundary(current);
                    state
                        .eager_initializers
                        .push((current, key_for_stack, boundary));
                }
                true
            } else {
                true
            }
        };
        // The registry is a persistent mutable GC root. Shade the new value
        // after releasing its lock so a full cycle that already scanned roots
        // cannot sweep a late-published exports object.
        if stored {
            crate::gc::runtime_write_barrier_root_nanbox(exports);
        }
        stored
    }

    /// Store the wrapper's final `module.exports`. Lazy modules remain owned by
    /// `require_with` until the generated init function returns (namespace
    /// population can still follow the CJS body). A wrapper running without an
    /// active lazy claim is eager, so this call is its completion boundary even
    /// if path lookup also registered the initializer's address.
    pub(super) fn register_final_exports(&self, key: String, exports: u64) -> bool {
        let current = std::thread::current().id();
        let (stored, completed_eager) = {
            let mut state = self.lock();
            let key_for_stack = key.clone();
            let (stored, completed_eager, finished_initializing) = {
                let entry = state.entries.entry(key).or_insert_with(|| PathModuleEntry {
                    init_addr: None,
                    exports: None,
                    status: PathModuleStatus::Registered,
                    active_claim: None,
                });
                if matches!(entry.status, PathModuleStatus::Failed(_))
                    || entry.active_claim.is_some_and(|owner| owner != current)
                    || matches!(entry.status, PathModuleStatus::Initializing(owner) if owner != current)
                {
                    (false, false, false)
                } else {
                    entry.exports = Some(exports);
                    // Every CJS wrapper publishes a partial/final pair. A final
                    // store completes an eager execution, even when an init
                    // address was also registered for path lookup. A lazy claim
                    // remains `Initializing` until its generated init returns, so
                    // a later namespace-population throw is still cached.
                    let completed_eager = entry.active_claim.is_none();
                    let finished_initializing = completed_eager
                        && matches!(entry.status, PathModuleStatus::Initializing(_));
                    if completed_eager {
                        entry.status = PathModuleStatus::Initialized;
                    }
                    (true, completed_eager, finished_initializing)
                }
            };
            if finished_initializing {
                state.finish_eager(current, &key_for_stack);
                state.finish_initializing(current);
            }
            (stored, completed_eager)
        };
        if stored {
            crate::gc::runtime_write_barrier_root_nanbox(exports);
        }
        if completed_eager {
            self.ready.notify_all();
        }
        stored
    }

    pub(super) fn begin_module_boundary(&self) -> u64 {
        let current = std::thread::current().id();
        let mut state = self.lock();
        state.next_module_boundary = state.next_module_boundary.wrapping_add(1).max(1);
        let boundary = state.next_module_boundary;
        state.module_boundaries.push((current, boundary));
        boundary
    }

    /// Leave one generated module body. A normal return with a still-partial
    /// eager CJS wrapper (for example, a legal top-level CommonJS `return`)
    /// completes with those partial exports. An exceptional return caches the
    /// original throw. Lazy claims are deliberately absent from this stack and
    /// are completed by `require_with` instead.
    pub(super) fn finish_module_boundary(&self, boundary: u64, error: Option<u64>) -> usize {
        let current = std::thread::current().id();
        let completed = {
            let mut state = self.lock();
            let boundary_index = state
                .module_boundaries
                .iter()
                .rposition(|(owner, candidate)| *owner == current && *candidate == boundary)
                .expect("generated module-init boundary stack became unbalanced");
            debug_assert!(
                state.module_boundaries[boundary_index + 1..]
                    .iter()
                    .all(|(owner, _)| *owner != current),
                "generated module-init boundaries must be LIFO per thread"
            );
            state.module_boundaries.remove(boundary_index);

            let mut keys = Vec::new();
            state.eager_initializers.retain(|(owner, key, candidate)| {
                if *owner == current && *candidate == Some(boundary) {
                    keys.push(key.clone());
                    false
                } else {
                    true
                }
            });
            for key in &keys {
                let entry = state
                    .entries
                    .get_mut(key)
                    .expect("eager path-module init entry disappeared while running");
                debug_assert!(entry.active_claim.is_none());
                debug_assert!(
                    matches!(entry.status, PathModuleStatus::Initializing(owner) if owner == current)
                );
                if let Some(error) = error {
                    entry.exports = None;
                    entry.status = PathModuleStatus::Failed(error);
                } else {
                    entry.status = PathModuleStatus::Initialized;
                }
            }
            for _ in 0..keys.len() {
                state.finish_initializing(current);
            }
            keys.len()
        };
        if completed > 0 {
            if let Some(error) = error {
                crate::gc::runtime_write_barrier_root_nanbox(error);
            }
            self.ready.notify_all();
        }
        completed
    }

    /// Return the value for `key`, initializing it once when necessary.
    ///
    /// The callback is invoked with every registry lock released. Its error is
    /// cached without retry and replayed to all waiters. `Ok(None)` is a miss;
    /// `Ok(Some(TAG_UNDEFINED))` is an initialized module exporting undefined.
    pub(super) fn require_with(
        &self,
        key: &str,
        initialize: &dyn Fn(usize) -> Result<(), u64>,
    ) -> Result<Option<u64>, PathModuleRequireError> {
        let current = std::thread::current().id();
        let init_addr = loop {
            let mut state = self.lock();
            if !self.adopt_registered_init(&mut state, key) {
                return Ok(None);
            }
            let Some(entry) = state.entries.get_mut(key) else {
                return Ok(None);
            };
            match entry.status {
                PathModuleStatus::Initialized => return Ok(entry.exports),
                PathModuleStatus::Failed(error) => {
                    return Err(PathModuleRequireError::Initializer(error));
                }
                PathModuleStatus::Initializing(owner) if owner == current => {
                    // CommonJS cycle: the owner sees its own partial exports.
                    return Ok(entry.exports);
                }
                PathModuleStatus::Initializing(_) => {
                    if state.loader_owner == Some(current) {
                        return Err(PathModuleRequireError::OwnershipConflict);
                    }
                    drop(
                        self.ready
                            .wait(state)
                            .unwrap_or_else(std::sync::PoisonError::into_inner),
                    );
                }
                PathModuleStatus::Registered => {
                    let Some(addr) = entry.init_addr else {
                        return Ok(entry.exports);
                    };
                    if state
                        .initializing_owner
                        .is_some_and(|owner| owner != current)
                    {
                        // An eager initializer started without a lazy-loader
                        // claim. Let it finish before granting ownership; it
                        // can recursively claim the loader itself without
                        // forming an A-waits-B / B-waits-A cycle.
                        drop(
                            self.ready
                                .wait(state)
                                .unwrap_or_else(std::sync::PoisonError::into_inner),
                        );
                        continue;
                    }
                    match state.loader_owner {
                        Some(owner) if owner != current => {
                            drop(
                                self.ready
                                    .wait(state)
                                    .unwrap_or_else(std::sync::PoisonError::into_inner),
                            );
                            continue;
                        }
                        Some(_) => state.loader_depth += 1,
                        None => {
                            state.loader_owner = Some(current);
                            state.loader_depth = 1;
                        }
                    }
                    let entry = state
                        .entries
                        .get_mut(key)
                        .expect("path entry disappeared while claiming loader ownership");
                    entry.status = PathModuleStatus::Initializing(current);
                    entry.active_claim = Some(current);
                    assert!(state.begin_initializing(current));
                    break addr;
                }
            }
        };

        // Never hold the registry lock across generated module code. That code
        // self-registers exports and may recursively require another path.
        let outcome = initialize(init_addr);
        let (result, failed_error) = {
            let mut state = self.lock();
            let entry = state
                .entries
                .get_mut(key)
                .expect("path initializer entry disappeared while it was running");
            debug_assert_eq!(entry.active_claim, Some(current));
            entry.active_claim = None;
            let (result, failed_error) = match outcome {
                Ok(()) => {
                    entry.status = PathModuleStatus::Initialized;
                    (Ok(entry.exports), None)
                }
                Err(error) => {
                    entry.exports = None;
                    entry.status = PathModuleStatus::Failed(error);
                    (Err(PathModuleRequireError::Initializer(error)), Some(error))
                }
            };
            state.finish_initializing(current);
            debug_assert_eq!(state.loader_owner, Some(current));
            debug_assert!(state.loader_depth > 0);
            state.loader_depth -= 1;
            if state.loader_depth == 0 {
                state.loader_owner = None;
            }
            (result, failed_error)
        };
        // Failed values are persistent roots too. Keep waiters asleep until
        // the cached exception has been shaded.
        if let Some(error) = failed_error {
            crate::gc::runtime_write_barrier_root_nanbox(error);
        }
        self.ready.notify_all();
        result
    }

    pub(super) fn has_exports(&self, key: &str) -> bool {
        self.published_exports(key).is_some()
    }

    /// Non-initializing read of a published value. A `Failed` entry reports a
    /// miss so the caller falls through to its own error path rather than
    /// handing back a value the initializer never finished producing.
    pub(super) fn published_exports(&self, key: &str) -> Option<u64> {
        let state = self.lock();
        state.entries.get(key).and_then(|entry| {
            if matches!(entry.status, PathModuleStatus::Failed(_)) {
                None
            } else {
                entry.exports
            }
        })
    }

    pub(super) fn scan_roots(&self, visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
        let mut state = crate::gc::lock_gc_root_registry(&self.state);
        for entry in state.entries.values_mut() {
            if let Some(exports) = entry.exports.as_mut() {
                visitor.visit_nanbox_u64_slot(exports);
            }
            if let PathModuleStatus::Failed(error) = &mut entry.status {
                visitor.visit_nanbox_u64_slot(error);
            }
        }
    }

    #[cfg(test)]
    pub(super) fn remove_for_test(&self, key: &str) {
        self.lock().entries.remove(key);
    }
}

// Canonical path -> generated initializer address, shared by every heap in
// the process. Holds only code addresses, so unlike the per-heap export table
// it is not a GC root and needs no scanner.
//
// `per_test_global!` because `perry-runtime`'s tests share one process: two
// tests registering DIFFERENT addresses for the same canonical path would
// otherwise collide, and the second would be rejected by the idempotence
// check below.
// `Option` because `HashMap::new` is not const and this must also compile as
// a plain `static` outside a test build.
//
// Plain `//`: a `///` here attaches to nothing the macro emits, so rustc drops
// it and `-D warnings` (a required PR gate) rejects the build. The rationale
// above is the point of the comment, so keep it — just not as rustdoc.
per_test_global!(
    static PATH_MODULE_INIT_ADDRS: std::sync::Mutex<
        Option<std::collections::HashMap<String, usize>>,
    > = std::sync::Mutex::new(None)
);

fn with_init_addrs<R>(f: impl FnOnce(&mut std::collections::HashMap<String, usize>) -> R) -> R {
    let mut guard = PATH_MODULE_INIT_ADDRS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(guard.get_or_insert_with(std::collections::HashMap::new))
}

/// Idempotent. A second, DIFFERENT address for one canonical path is rejected
/// so an alias can never create a second logical module initialization.
fn register_init_addr(key: &str, init_addr: usize) -> bool {
    with_init_addrs(|addrs| match addrs.entry(key.to_string()) {
        std::collections::hash_map::Entry::Occupied(slot) => *slot.get() == init_addr,
        std::collections::hash_map::Entry::Vacant(slot) => {
            slot.insert(init_addr);
            true
        }
    })
}

fn lookup_init_addr(key: &str) -> Option<usize> {
    with_init_addrs(|addrs| addrs.get(key).copied())
}

crate::perry_thread_local! {
    /// Per-heap, not per-process. `PathModuleEntry::exports` holds NaN-boxed
    /// pointers into the arena of the thread that produced them, and both the
    /// arena and the mutable-root scanners are thread-local — so a
    /// process-global table would hand one heap's pointer to another heap's
    /// collector. One table per runtime thread keeps every entry's referent in
    /// the same heap as the scanner that rewrites it, which is what lets a
    /// single process host more than one Perry application.
    ///
    /// Keyed by THREAD, not by [`crate::agent::AgentId`], even though #6294
    /// established agent tagging for the cross-thread queues. The two are
    /// keyed differently on purpose:
    ///
    /// - An agent tags *queued work* so a drain can tell whose pointers it may
    ///   touch. A thread with no agent of its own resolves to `PRIMARY_AGENT`
    ///   because it is a pump acting for the primary heap.
    /// - This table instead holds pointers into an *arena*, and the arena is
    ///   itself a `thread_local!` (`thread.rs`). Thread is therefore exactly
    ///   the domain those pointers live and die in.
    ///
    /// The distinction is load-bearing for embedders: a Coop app thread is a
    /// plain `std::thread::spawn`, not a `perry/thread` worker, so it never
    /// calls `enter_worker_agent()` and every such thread resolves to
    /// `PRIMARY_AGENT`. Agent-keying would hand all of a host's apps one
    /// shared table — the bug this replaced, minus the guard that used to make
    /// it loud.
    pub(super) static MODULE_PATH_REGISTRY: PathModuleRegistry = PathModuleRegistry::default();
}

#[cfg(test)]
mod path_module_registry_tests {
    use super::super::canonicalize_module_path;
    use super::*;
    use crate::value::TAG_UNDEFINED;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc, Barrier, Condvar, Mutex,
    };

    struct InitGate {
        entered: mpsc::Sender<usize>,
        released: Mutex<std::collections::HashSet<usize>>,
        ready: Condvar,
    }

    impl InitGate {
        fn enter_and_wait(&self, addr: usize) {
            self.entered.send(addr).unwrap();
            let mut released = self.released.lock().unwrap();
            while !released.contains(&addr) {
                released = self.ready.wait(released).unwrap();
            }
        }

        fn release(&self, addr: usize) {
            self.released.lock().unwrap().insert(addr);
            self.ready.notify_all();
        }
    }

    fn initialize_two_key_cycle(
        registry: &PathModuleRegistry,
        gate: &InitGate,
        calls: &[AtomicUsize; 2],
        partial_observations: &AtomicUsize,
        addr: usize,
    ) -> Result<(), u64> {
        gate.enter_and_wait(addr);
        let (key, other_key, partial, other_partial, final_value, call_index) = match addr {
            31 => ("a.js", "b.js", 0xA1, 0xB1, 0xA2, 0),
            37 => ("b.js", "a.js", 0xB1, 0xA1, 0xB2, 1),
            _ => panic!("unexpected initializer address {addr}"),
        };
        calls[call_index].fetch_add(1, Ordering::Relaxed);
        assert!(registry.register_partial_exports(key.into(), partial));
        let observed = registry.require_with(other_key, &|next_addr| {
            initialize_two_key_cycle(registry, gate, calls, partial_observations, next_addr)
        });
        let observed = match observed {
            Ok(value) => value,
            Err(PathModuleRequireError::Initializer(error)) => return Err(error),
            Err(PathModuleRequireError::OwnershipConflict) => {
                panic!("loader serialization admitted a cross-owner cycle")
            }
        };
        if observed == Some(other_partial) {
            partial_observations.fetch_add(1, Ordering::Relaxed);
        }
        assert!(registry.register_final_exports(key.into(), final_value));
        Ok(())
    }

    #[test]
    fn recursive_load_observes_partial_exports_without_reentering_init() {
        let registry = PathModuleRegistry::default();
        assert!(registry.register_init("route.js".into(), 7));
        let calls = AtomicUsize::new(0);

        let result = registry
            .require_with("route.js", &|addr| {
                assert_eq!(addr, 7);
                calls.fetch_add(1, Ordering::Relaxed);
                assert!(registry.register_partial_exports("route.js".into(), 0xA1));
                let recursive = registry.require_with("route.js", &|_| {
                    panic!("recursive load must not execute the initializer")
                });
                let recursive =
                    recursive.expect("same-owner recursive load must return partial exports");
                assert_eq!(recursive, Some(0xA1));
                assert!(registry.register_final_exports("route.js".into(), 0xA2));
                Ok(())
            })
            .unwrap();

        assert_eq!(result, Some(0xA2));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn concurrent_first_load_runs_one_initializer_and_publishes_one_value() {
        const THREADS: usize = 20;
        let registry = Arc::new(PathModuleRegistry::default());
        assert!(registry.register_init("chunk.js".into(), 11));
        let starts = Arc::new(Barrier::new(THREADS + 1));
        let init_entered = Arc::new(Barrier::new(2));
        let release_init = Arc::new(Barrier::new(2));
        let calls = Arc::new(AtomicUsize::new(0));

        let mut workers = Vec::new();
        for _ in 0..THREADS {
            let registry = Arc::clone(&registry);
            let starts = Arc::clone(&starts);
            let init_entered = Arc::clone(&init_entered);
            let release_init = Arc::clone(&release_init);
            let calls = Arc::clone(&calls);
            workers.push(std::thread::spawn(move || {
                starts.wait();
                registry.require_with("chunk.js", &|addr| {
                    assert_eq!(addr, 11);
                    calls.fetch_add(1, Ordering::Relaxed);
                    assert!(registry.register_partial_exports("chunk.js".into(), 0xB1));
                    init_entered.wait();
                    release_init.wait();
                    assert!(registry.register_final_exports("chunk.js".into(), 0xB2));
                    Ok(())
                })
            }));
        }

        starts.wait();
        init_entered.wait();
        release_init.wait();
        for worker in workers {
            assert_eq!(worker.join().unwrap().unwrap(), Some(0xB2));
        }
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn opposite_concurrent_cycle_serializes_generated_init_and_completes() {
        let registry = Arc::new(PathModuleRegistry::default());
        assert!(registry.register_init("a.js".into(), 31));
        assert!(registry.register_init("b.js".into(), 37));
        let starts = Arc::new(Barrier::new(3));
        let (entered_tx, entered_rx) = mpsc::channel();
        let gate = Arc::new(InitGate {
            entered: entered_tx,
            released: Mutex::new(std::collections::HashSet::new()),
            ready: Condvar::new(),
        });
        let calls = Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)]);
        let partial_observations = Arc::new(AtomicUsize::new(0));
        let (result_tx, result_rx) = mpsc::channel();

        let mut workers = Vec::new();
        for (key, expected) in [("a.js", 0xA2), ("b.js", 0xB2)] {
            let registry = Arc::clone(&registry);
            let starts = Arc::clone(&starts);
            let gate = Arc::clone(&gate);
            let calls = Arc::clone(&calls);
            let partial_observations = Arc::clone(&partial_observations);
            let result_tx = result_tx.clone();
            workers.push(std::thread::spawn(move || {
                starts.wait();
                let result = registry.require_with(key, &|addr| {
                    initialize_two_key_cycle(&registry, &gate, &calls, &partial_observations, addr)
                });
                result_tx.send((expected, result)).unwrap();
            }));
        }
        drop(result_tx);

        starts.wait();
        let first = entered_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("one thread must claim the logical loader");
        assert!(
            entered_rx
                .recv_timeout(std::time::Duration::from_millis(200))
                .is_err(),
            "opposite keys ran generated init concurrently and can deadlock"
        );
        gate.release(first);
        let nested = entered_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("the loader owner must recursively initialize the opposite key");
        assert_ne!(nested, first);
        gate.release(nested);

        for _ in 0..2 {
            let (expected, result) = result_rx
                .recv_timeout(std::time::Duration::from_secs(2))
                .expect("A <-> B concurrent load did not complete within the bound");
            assert_eq!(result.unwrap(), Some(expected));
        }
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(calls[0].load(Ordering::Relaxed), 1);
        assert_eq!(calls[1].load(Ordering::Relaxed), 1);
        assert_eq!(partial_observations.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn foreign_partial_publication_is_rejected_while_loader_is_owned() {
        let registry = Arc::new(PathModuleRegistry::default());
        assert!(registry.register_init("a.js".into(), 41));
        assert!(registry.register_init("b.js".into(), 43));
        let (entered_tx, entered_rx) = mpsc::channel();
        let release = Arc::new(Barrier::new(2));

        let worker_registry = Arc::clone(&registry);
        let worker_release = Arc::clone(&release);
        let worker = std::thread::spawn(move || {
            worker_registry.require_with("a.js", &|addr| {
                assert_eq!(addr, 41);
                assert!(worker_registry.register_partial_exports("a.js".into(), 0xA1));
                entered_tx.send(()).unwrap();
                worker_release.wait();
                assert_eq!(
                    worker_registry.require_with("b.js", &|nested_addr| {
                        assert_eq!(nested_addr, 43);
                        assert!(worker_registry.register_partial_exports("b.js".into(), 0xB1));
                        assert!(worker_registry.register_final_exports("b.js".into(), 0xB2));
                        Ok(())
                    }),
                    Ok(Some(0xB2))
                );
                assert!(worker_registry.register_final_exports("a.js".into(), 0xA2));
                Ok(())
            })
        });

        entered_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("loader owner did not enter generated initialization");
        assert!(
            !registry.register_partial_exports("b.js".into(), 0xDEAD),
            "a foreign initializer must not publish while the loader is owned"
        );
        release.wait();
        assert_eq!(worker.join().unwrap(), Ok(Some(0xA2)));
    }

    #[test]
    fn registered_initializer_can_complete_through_an_eager_wrapper_pair() {
        let registry = PathModuleRegistry::default();
        assert!(registry.register_init("eager.js".into(), 47));
        assert!(registry.register_partial_exports("eager.js".into(), 0xE1));
        assert!(registry.register_final_exports("eager.js".into(), 0xE2));
        assert_eq!(
            registry.require_with("eager.js", &|_| panic!(
                "eager module must already be complete"
            )),
            Ok(Some(0xE2))
        );
    }

    #[test]
    fn eager_wrapper_throw_replaces_partial_exports_and_wakes_waiters() {
        let registry = Arc::new(PathModuleRegistry::default());
        assert!(registry.register_init("eager-throws.js".into(), 49));
        let boundary = registry.begin_module_boundary();
        assert!(registry.register_partial_exports("eager-throws.js".into(), 0xE1));

        let (waiting_tx, waiting_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let waiter_registry = Arc::clone(&registry);
        let waiter = std::thread::spawn(move || {
            waiting_tx.send(()).unwrap();
            let result = waiter_registry.require_with("eager-throws.js", &|_| {
                panic!("a waiter must not retry the eager initializer")
            });
            result_tx.send(result).unwrap();
        });
        waiting_rx.recv().unwrap();
        assert!(
            result_rx
                .recv_timeout(std::time::Duration::from_millis(100))
                .is_err(),
            "the foreign waiter returned partial exports instead of waiting"
        );

        assert_eq!(registry.finish_module_boundary(boundary, Some(0xBAD)), 1);
        assert_eq!(
            result_rx
                .recv_timeout(std::time::Duration::from_secs(2))
                .expect("eager failure did not wake the waiting require"),
            Err(PathModuleRequireError::Initializer(0xBAD))
        );
        waiter.join().unwrap();
        assert_eq!(
            registry.require_with("eager-throws.js", &|_| {
                panic!("failed eager module must not retry")
            }),
            Err(PathModuleRequireError::Initializer(0xBAD))
        );
        let state = registry.lock();
        assert_eq!(state.initializing_count, 0);
        assert_eq!(state.initializing_owner, None);
    }

    #[test]
    fn caught_nested_eager_failure_does_not_poison_outer_wrapper() {
        let registry = PathModuleRegistry::default();
        assert!(registry.register_init("outer.js".into(), 51));
        assert!(registry.register_init("inner.js".into(), 53));
        let outer_boundary = registry.begin_module_boundary();
        assert!(registry.register_partial_exports("outer.js".into(), 0xA1));
        let inner_boundary = registry.begin_module_boundary();
        assert!(registry.register_partial_exports("inner.js".into(), 0xB1));

        assert_eq!(
            registry.finish_module_boundary(inner_boundary, Some(0xBAD)),
            1
        );
        assert!(registry.register_final_exports("outer.js".into(), 0xA2));
        assert_eq!(registry.finish_module_boundary(outer_boundary, None), 0);
        assert_eq!(
            registry.require_with("inner.js", &|_| panic!("inner failure must be cached")),
            Err(PathModuleRequireError::Initializer(0xBAD))
        );
        assert_eq!(
            registry.require_with("outer.js", &|_| panic!("outer module completed eagerly")),
            Ok(Some(0xA2))
        );
        let state = registry.lock();
        assert_eq!(state.initializing_count, 0);
        assert!(state.eager_initializers.is_empty());
    }

    #[test]
    fn lazy_inner_boundary_does_not_consume_outer_eager_wrapper() {
        let registry = PathModuleRegistry::default();
        assert!(registry.register_init("outer.js".into(), 55));
        let outer_boundary = registry.begin_module_boundary();
        assert!(registry.register_partial_exports("outer.js".into(), 0xA1));

        // A lazily claimed inner module does not enter the eager stack. Its
        // native catch must leave the outer wrapper available to catch or
        // complete independently.
        let inner_boundary = registry.begin_module_boundary();
        assert_eq!(
            registry.finish_module_boundary(inner_boundary, Some(0xBAD)),
            0
        );
        assert!(registry.register_final_exports("outer.js".into(), 0xA2));
        assert_eq!(registry.finish_module_boundary(outer_boundary, None), 0);
        assert_eq!(
            registry.require_with("outer.js", &|_| panic!("outer module completed eagerly")),
            Ok(Some(0xA2))
        );
    }

    #[test]
    fn eager_top_level_return_completes_with_partial_exports() {
        let registry = PathModuleRegistry::default();
        assert!(registry.register_init("returns.js".into(), 57));
        let boundary = registry.begin_module_boundary();
        assert!(registry.register_partial_exports("returns.js".into(), 0xC1));
        assert_eq!(registry.finish_module_boundary(boundary, None), 1);
        assert_eq!(
            registry.require_with("returns.js", &|_| {
                panic!("a normally returned eager wrapper must not retry")
            }),
            Ok(Some(0xC1))
        );
    }

    /// Each runtime thread owns its own table. An entry published on one
    /// thread must be invisible to another: `exports` is a NaN-boxed pointer
    /// into the publishing thread's arena, and only that thread's scanner
    /// rewrites it. This is what lets one process host several Perry apps.
    #[test]
    fn path_registry_is_per_heap_not_per_process() {
        const KEY: &str = "/per-heap-isolation-fixture.js";
        MODULE_PATH_REGISTRY.with(|registry| {
            assert!(registry.register_final_exports(KEY.into(), 0xD1));
            assert_eq!(registry.published_exports(KEY), Some(0xD1));
        });

        let seen_by_second_heap = std::thread::spawn(|| {
            MODULE_PATH_REGISTRY.with(|registry| registry.published_exports(KEY))
        })
        .join()
        .unwrap();
        assert_eq!(
            seen_by_second_heap, None,
            "a second heap must not observe the first heap's exports pointer"
        );

        // The publishing thread still sees its own entry: isolation, not loss.
        MODULE_PATH_REGISTRY.with(|registry| {
            assert_eq!(registry.published_exports(KEY), Some(0xD1));
            registry.remove_for_test(KEY);
        });
    }

    /// The Coop shape: many apps in one process load the SAME module path.
    /// Each heap must run its own initializer and get its own exports, with
    /// no thread refused and no initializer skipped as already-done.
    #[test]
    fn every_heap_runs_its_own_initializer_for_the_same_path() {
        const KEY: &str = "/shared-bundle-path.js";
        fn init_on_this_heap(marker: u64) -> Option<u64> {
            // Seeds only the process-wide ADDRESS, never an entry, so the
            // require below has to reach `adopt_registered_init` — the path a
            // second heap actually takes. Calling `register_init` here instead
            // would insert the entry directly and skip the very code under
            // test; an earlier version of this test did exactly that and so
            // could not see that a second heap had no initializer to run.
            //
            // Seeding per thread is a TEST-BUILD requirement, not a
            // production one: `per_test_global!` expands to a per-thread
            // instance under `cfg(test)`, whereas the shipped static is one
            // process-wide map that codegen fills once at startup.
            assert!(register_init_addr(KEY, 0x2000));
            MODULE_PATH_REGISTRY.with(|registry| {
                let ran = std::cell::Cell::new(false);
                let outcome = registry.require_with(KEY, &|_addr| {
                    ran.set(true);
                    assert!(registry.register_final_exports(KEY.into(), marker));
                    Ok(())
                });
                assert!(ran.get(), "this heap's initializer must actually run");
                outcome.expect("no heap may be refused ownership")
            })
        }

        let first = init_on_this_heap(0xA1);
        let second = std::thread::spawn(|| init_on_this_heap(0xB2))
            .join()
            .unwrap();

        assert_eq!(first, Some(0xA1));
        assert_eq!(
            second,
            Some(0xB2),
            "the second heap got the first heap's value"
        );
        MODULE_PATH_REGISTRY.with(|registry| registry.remove_for_test(KEY));
    }

    #[test]
    fn undefined_export_is_present_and_distinct_from_a_miss() {
        let registry = PathModuleRegistry::default();
        assert!(registry.register_init("undefined.js".into(), 13));
        let value = registry
            .require_with("undefined.js", &|_| {
                assert!(registry.register_partial_exports("undefined.js".into(), TAG_UNDEFINED));
                assert!(registry.register_final_exports("undefined.js".into(), TAG_UNDEFINED));
                Ok(())
            })
            .unwrap();

        assert_eq!(value, Some(TAG_UNDEFINED));
        assert!(registry.has_exports("undefined.js"));
        assert_eq!(
            registry.require_with("missing.js", &|_| unreachable!()),
            Ok(None)
        );
    }

    #[test]
    fn concurrent_initialization_failure_is_shared_and_cached_without_retry() {
        const THREADS: usize = 20;
        let registry = Arc::new(PathModuleRegistry::default());
        assert!(registry.register_init("throws.js".into(), 17));
        let starts = Arc::new(Barrier::new(THREADS + 1));
        let init_entered = Arc::new(Barrier::new(2));
        let release_init = Arc::new(Barrier::new(2));
        let calls = Arc::new(AtomicUsize::new(0));
        let error = 0x7FFD_0000_0000_0042;

        let mut workers = Vec::new();
        for _ in 0..THREADS {
            let registry = Arc::clone(&registry);
            let starts = Arc::clone(&starts);
            let init_entered = Arc::clone(&init_entered);
            let release_init = Arc::clone(&release_init);
            let calls = Arc::clone(&calls);
            workers.push(std::thread::spawn(move || {
                starts.wait();
                registry.require_with("throws.js", &|addr| {
                    assert_eq!(addr, 17);
                    calls.fetch_add(1, Ordering::Relaxed);
                    init_entered.wait();
                    release_init.wait();
                    Err(error)
                })
            }));
        }

        starts.wait();
        init_entered.wait();
        release_init.wait();
        for worker in workers {
            assert_eq!(
                worker.join().unwrap(),
                Err(PathModuleRequireError::Initializer(error))
            );
        }
        assert_eq!(
            registry.require_with("throws.js", &|_| {
                panic!("failed path modules use the explicit no-retry policy")
            }),
            Err(PathModuleRequireError::Initializer(error))
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn canonical_alias_cannot_replace_the_first_initializer() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp = std::env::temp_dir().join(format!(
            "perry-path-module-alias-{}-{nonce}",
            std::process::id()
        ));
        let nested = temp.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let module = temp.join("module.js");
        std::fs::write(&module, "module.exports = 1;").unwrap();
        let direct = canonicalize_module_path(&module.to_string_lossy());
        let aliased =
            canonicalize_module_path(&nested.join("..").join("module.js").to_string_lossy());
        assert_eq!(direct, aliased);

        let registry = PathModuleRegistry::default();
        assert!(registry.register_init(direct.clone(), 19));
        assert!(!registry.register_init(aliased, 23));
        let seen = AtomicUsize::new(0);
        assert_eq!(
            registry.require_with(&direct, &|addr| {
                seen.store(addr, Ordering::Relaxed);
                assert!(registry.register_final_exports(direct.clone(), 0xC1));
                Ok(())
            }),
            Ok(Some(0xC1))
        );
        assert_eq!(seen.load(Ordering::Relaxed), 19);
        std::fs::remove_dir_all(temp).unwrap();
    }
}
