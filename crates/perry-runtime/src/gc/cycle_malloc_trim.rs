//! System trim and major-only native allocator purge. Idle hooks use the same
//! native purge after collection scratch has been dropped.

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct MallocTrimOutcome {
    pub(super) status: AllocatorMaintenanceStatus,
    pub(super) reason: AllocatorMaintenanceReason,
    pub(super) elapsed_us: u64,
}

#[cfg(test)]
thread_local! {
    static TEST_MALLOC_TRIM_CALLS: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_test_malloc_trim_call_count() {
    TEST_MALLOC_TRIM_CALLS.with(|calls| calls.set(0));
}

#[cfg(test)]
pub(crate) fn test_malloc_trim_call_count() -> usize {
    TEST_MALLOC_TRIM_CALLS.with(Cell::get)
}

#[cfg(test)]
fn record_test_malloc_trim_call() {
    TEST_MALLOC_TRIM_CALLS.with(|calls| calls.set(calls.get().saturating_add(1)));
}

// Two counters, because they are two different claims and only one of them is
// portable. `..._CALLS` witnesses that budgeted reclaim REACHED the trim call —
// #6180's actual subject, since the bug was `ordinary_budgeted` skipping it —
// and holds on every target. `..._EXECUTED` witnesses that a trim primitive
// actually ran, which is only meaningful where one exists.
//
// Counting only the executing arms made the gate unsatisfiable on Windows and
// musl (#7356). Counting only reaches would have quietly dropped the stronger
// property on glibc/macOS, where nothing today separates reaching from
// executing but a future early return would. Keeping both means neither
// platform's gate asserts something it cannot see, and neither asserts less
// than it could.
#[cfg(all(test, any(target_env = "gnu", target_os = "macos")))]
thread_local! {
    static TEST_MALLOC_TRIM_EXECUTED: Cell<usize> = const { Cell::new(0) };
}

#[cfg(all(test, any(target_env = "gnu", target_os = "macos")))]
pub(crate) fn reset_test_malloc_trim_executed_count() {
    TEST_MALLOC_TRIM_EXECUTED.with(|calls| calls.set(0));
}

#[cfg(all(test, any(target_env = "gnu", target_os = "macos")))]
pub(crate) fn test_malloc_trim_executed_count() -> usize {
    TEST_MALLOC_TRIM_EXECUTED.with(Cell::get)
}

#[cfg(all(test, any(target_env = "gnu", target_os = "macos")))]
fn record_test_malloc_trim_executed() {
    TEST_MALLOC_TRIM_EXECUTED.with(|calls| calls.set(calls.get().saturating_add(1)));
}

/// Purge the allocator the process ACTUALLY allocates through.
///
/// [`run_malloc_trim`] addresses glibc (`malloc_trim`) or the macOS malloc
/// zones (`malloc_zone_pressure_relief`). Neither reaches mimalloc, which
/// `lib.rs` installs as the `#[global_allocator]` on every 64-bit target — so
/// on both production platforms the GC's hand-back step was trimming an
/// allocator almost nothing in the process uses. Measured on the compiled
/// claude-code TUI (Linux, idle): three explicit full collections each ran
/// `malloc_trim(0)` in 16-23 us and moved RSS by 0 MB, while the same
/// collection under `MIMALLOC_PURGE_DELAY=0` returned 318 MB. The collector
/// was already freeing the memory; mimalloc was holding the pages.
///
/// `mi_collect(force = true)` is the mimalloc equivalent: it collects the
/// thread's heaps and returns abandoned/free segments to the OS. It runs at
/// the same point as the trim (Reclaim, outside the atomic tail) and only for
/// MAJOR collections, so its cost is paid once per full cycle rather than on
/// every free the way the `MIMALLOC_PURGE_DELAY=0` knob would.
///
/// Kill switch: `PERRY_GC_MALLOC_PURGE=0` (also `off`/`false`).
#[inline(never)]
pub(super) fn run_allocator_purge(major: bool) -> MallocTrimOutcome {
    if !major {
        return MallocTrimOutcome {
            status: AllocatorMaintenanceStatus::Skipped,
            reason: AllocatorMaintenanceReason::MinorCollection,
            elapsed_us: 0,
        };
    }
    if !allocator_purge_enabled() {
        return MallocTrimOutcome {
            status: AllocatorMaintenanceStatus::Skipped,
            reason: AllocatorMaintenanceReason::Disabled,
            elapsed_us: 0,
        };
    }
    #[cfg(all(target_pointer_width = "64", feature = "alloc-mimalloc"))]
    {
        let start = Instant::now();
        // SAFETY: `mi_collect` is mimalloc's own public maintenance entry. It
        // takes no pointer from us and is safe to call at any time from a
        // thread that allocates; the collector owns the heap here, and this
        // touches allocator metadata only, never a JS object.
        #[cfg(test)]
        TEST_MIMALLOC_PURGES.with(|c| c.set(c.get().saturating_add(1)));
        unsafe {
            libmimalloc_sys::mi_collect(true);
        }
        return MallocTrimOutcome {
            status: AllocatorMaintenanceStatus::Executed,
            reason: AllocatorMaintenanceReason::ExplicitOrEmergency,
            elapsed_us: start.elapsed().as_micros() as u64,
        };
    }
    #[cfg(not(all(target_pointer_width = "64", feature = "alloc-mimalloc")))]
    {
        MallocTrimOutcome {
            status: AllocatorMaintenanceStatus::Unsupported,
            reason: AllocatorMaintenanceReason::NotSupported,
            elapsed_us: 0,
        }
    }
}

/// `PERRY_GC_BLOCK_PERSIST_ALWAYS` — OFF by default. Set it to run the
/// block-persistence pass on every cycle again, i.e. the behaviour before
/// #9628 taught it to skip cycles whose root set is complete. Bisection knob;
/// see `cycle::GcCycleState::block_persistence_is_redundant`.
pub(super) fn block_persist_always_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *crate::once_init::get_or_init(&ENABLED, || {
        crate::gc::env_flag_enabled("PERRY_GC_BLOCK_PERSIST_ALWAYS")
    })
}

/// `PERRY_GC_MALLOC_PURGE` — ON by default; `=0`/`off`/`false` disables the
/// mimalloc purge above and restores the pre-#9612 behaviour.
fn allocator_purge_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *crate::once_init::get_or_init(&ENABLED, || {
        crate::gc::env_default_on_enabled("PERRY_GC_MALLOC_PURGE")
    })
}

pub(super) fn run_malloc_trim(_progress_kind: GcProgressKind) -> MallocTrimOutcome {
    // #6179/#6180 RSS floor: budgeted cycles are the DEFAULT-path collector
    // once incremental graduates — skipping allocator trim there meant a
    // long-lived incremental process never returned freed allocator pages to
    // the OS (2026-07-09 audit finding). Trim runs at Reclaim, outside the
    // atomic tail, and is itself bounded allocator maintenance.

    // The test counter records that budgeted reclaim REACHED this call (the
    // #6180 subject — the old bug was skipping it with `ordinary_budgeted`),
    // not that the platform executed a trim: on targets with no trim
    // primitive (Windows, musl) the outcome below is `Unsupported`, and
    // counting only the executing arms made the gate impossible to satisfy
    // there (#7356).
    #[cfg(test)]
    record_test_malloc_trim_call();

    let trim_start = Instant::now();
    // Native pages belong to run_allocator_purge, which is major-only.
    // Purging them here discards reusable GC scratch on nonmoving minors
    // and purges the same heap twice on full collections.

    #[cfg(target_env = "gnu")]
    {
        #[cfg(test)]
        record_test_malloc_trim_executed();

        unsafe {
            libc::malloc_trim(0);
        }
        return MallocTrimOutcome {
            status: AllocatorMaintenanceStatus::Executed,
            reason: AllocatorMaintenanceReason::ExplicitOrEmergency,
            elapsed_us: trim_start.elapsed().as_micros() as u64,
        };
    }

    #[cfg(target_os = "macos")]
    {
        #[cfg(test)]
        record_test_malloc_trim_executed();

        // Darwin counterpart of glibc's malloc_trim: ask every malloc zone
        // to return clean pages to the OS. Bounded allocator maintenance —
        // same placement (Reclaim, outside the atomic tail).
        unsafe extern "C" {
            fn malloc_zone_pressure_relief(zone: *mut core::ffi::c_void, goal: usize) -> usize;
        }
        unsafe {
            // NULL zone = all zones; goal 0 = release as much as possible.
            malloc_zone_pressure_relief(core::ptr::null_mut(), 0);
        }
        return MallocTrimOutcome {
            status: AllocatorMaintenanceStatus::Executed,
            reason: AllocatorMaintenanceReason::ExplicitOrEmergency,
            elapsed_us: trim_start.elapsed().as_micros() as u64,
        };
    }

    #[cfg(not(any(target_env = "gnu", target_os = "macos")))]
    {
        MallocTrimOutcome {
            status: AllocatorMaintenanceStatus::Unsupported,
            reason: AllocatorMaintenanceReason::NotSupported,
            elapsed_us: 0,
        }
    }
}

#[cfg(test)]
thread_local! {
    static TEST_MIMALLOC_PURGES: Cell<usize> = const { Cell::new(0) };
}

/// How many times this thread's reclaim tails reached the mimalloc purge. A
/// reach witness in the `..._CALLS` sense: on a target without the
/// `alloc-mimalloc` allocator it stays at zero and the gate that reads it must
/// say so rather than pass vacuously.
#[cfg(test)]
pub(crate) fn test_mimalloc_purge_count() -> usize {
    TEST_MIMALLOC_PURGES.with(Cell::get)
}

#[cfg(test)]
pub(crate) fn reset_test_mimalloc_purge_count() {
    TEST_MIMALLOC_PURGES.with(|c| c.set(0));
}

/// Whether the reclaim tail's allocator purge reaches mimalloc on this build.
#[cfg(test)]
pub(crate) const fn mimalloc_purge_available() -> bool {
    cfg!(all(target_pointer_width = "64", feature = "alloc-mimalloc"))
}

/// Apple GC blocks are their own regions (`arena/region/apple.rs`), so block
/// allocation no longer passes through mimalloc. mimalloc runs its deferred
/// purges on its own allocation traffic, and GC block churn used to be much of
/// it: freed collector and runtime memory then stayed dirty until the next
/// full collection's [`run_allocator_purge`] (+5 MiB peak footprint on an
/// Effect workload, where block allocations had been reusing that memory).
///
/// Drain those purges when the GC heap grows by a fresh region, at most once
/// per mimalloc's default purge delay on each thread, so memory freed since
/// the last drain leaves the footprint before the heap takes new pages.
/// `PERRY_GC_MALLOC_PURGE=0` disables this along with the cycle purge.
#[cfg(target_vendor = "apple")]
pub(crate) fn purge_allocator_on_region_growth() {
    thread_local! {
        static LAST_REGION_GROWTH_PURGE: Cell<Option<Instant>> = const { Cell::new(None) };
    }
    #[cfg(test)]
    TEST_REGION_GROWTH_PURGE_CHECKS.with(|c| c.set(c.get().saturating_add(1)));
    if !allocator_purge_enabled() {
        return;
    }
    if !LAST_REGION_GROWTH_PURGE.with(|last| region_growth_purge_due(last, Instant::now())) {
        return;
    }
    #[cfg(all(target_pointer_width = "64", feature = "alloc-mimalloc"))]
    // SAFETY: as in `run_allocator_purge`: mimalloc's own maintenance entry,
    // called from a thread that allocates; it touches allocator metadata only.
    unsafe {
        libmimalloc_sys::mi_collect(true);
    }
}

/// mimalloc's default `purge_delay`.
#[cfg(target_vendor = "apple")]
const REGION_GROWTH_PURGE_INTERVAL: Duration = Duration::from_millis(10);

#[cfg(target_vendor = "apple")]
fn region_growth_purge_due(last: &Cell<Option<Instant>>, now: Instant) -> bool {
    if last
        .get()
        .is_some_and(|then| now.saturating_duration_since(then) < REGION_GROWTH_PURGE_INTERVAL)
    {
        return false;
    }
    last.set(Some(now));
    true
}

#[cfg(all(test, target_vendor = "apple"))]
thread_local! {
    static TEST_REGION_GROWTH_PURGE_CHECKS: Cell<usize> = const { Cell::new(0) };
}

#[cfg(all(test, target_vendor = "apple"))]
pub(crate) fn test_region_growth_purge_checks() -> usize {
    TEST_REGION_GROWTH_PURGE_CHECKS.with(Cell::get)
}

#[cfg(all(test, target_vendor = "apple"))]
mod region_growth_purge_tests {
    use super::*;

    #[test]
    fn region_growth_purges_at_most_once_per_purge_delay() {
        let last = Cell::new(None);
        let start = Instant::now();
        assert!(region_growth_purge_due(&last, start), "first growth drains");
        assert!(!region_growth_purge_due(
            &last,
            start + Duration::from_millis(1)
        ));
        assert!(!region_growth_purge_due(
            &last,
            start + REGION_GROWTH_PURGE_INTERVAL - Duration::from_micros(1)
        ));
        assert!(region_growth_purge_due(
            &last,
            start + REGION_GROWTH_PURGE_INTERVAL
        ));
        assert!(!region_growth_purge_due(
            &last,
            start + REGION_GROWTH_PURGE_INTERVAL + Duration::from_millis(1)
        ));
    }

    #[test]
    fn fresh_region_maps_check_for_a_due_purge() {
        let before = test_region_growth_purge_checks();
        unsafe {
            let data = crate::arena::new_test_block_backing(crate::arena::BLOCK_SIZE);
            assert!(!data.is_null());
            crate::arena::release_test_block_backing(data, crate::arena::BLOCK_SIZE);
        }
        assert_eq!(test_region_growth_purge_checks(), before + 1);
    }
}

#[cfg(test)]
mod reclaim_purge_tests {
    use super::*;

    #[test]
    fn system_trim_does_not_purge_native_pages_between_minor_cycles() {
        reset_test_mimalloc_purge_count();
        let _ = run_malloc_trim(GcProgressKind::NormalIncremental);
        assert_eq!(
            test_mimalloc_purge_count(),
            0,
            "system trim must leave the GC allocator's reusable pages resident"
        );
    }

    #[test]
    fn minor_native_purge_is_a_negative_control() {
        reset_test_mimalloc_purge_count();
        let outcome = run_allocator_purge(false);
        assert_eq!(outcome.status, AllocatorMaintenanceStatus::Skipped);
        assert_eq!(outcome.reason, AllocatorMaintenanceReason::MinorCollection);
        assert_eq!(test_mimalloc_purge_count(), 0);
    }

    #[test]
    fn a_real_full_collection_reaches_one_native_purge() {
        std::thread::spawn(|| {
            // Initialize the providers, then count the next real collection.
            crate::gc::js_gc_collect();
            reset_test_mimalloc_purge_count();
            crate::gc::js_gc_collect();
            assert_eq!(
                test_mimalloc_purge_count(),
                usize::from(mimalloc_purge_available())
            );
        })
        .join()
        .unwrap();
    }
}
