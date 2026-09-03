//! `malloc_trim` maintenance for a collection cycle, split out of
//! `cycle.rs` for the 2000-line cap. Unchanged; only its home moved.

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

/// `PERRY_GC_MALLOC_PURGE` — ON by default; `=0`/`off`/`false` disables the
/// mimalloc purge above and restores the pre-#9612 behaviour.
fn allocator_purge_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| crate::gc::env_default_on_enabled("PERRY_GC_MALLOC_PURGE"))
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
    // Only the no-platform-trim arm below reads the flag; glibc and Darwin
    // report `Executed` from their own primitive.
    #[cfg_attr(any(target_env = "gnu", target_os = "macos"), allow(unused_variables))]
    let purged_mimalloc = purge_mimalloc_cache();

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
        if purged_mimalloc {
            MallocTrimOutcome {
                status: AllocatorMaintenanceStatus::Executed,
                reason: AllocatorMaintenanceReason::ExplicitOrEmergency,
                elapsed_us: trim_start.elapsed().as_micros() as u64,
            }
        } else {
            MallocTrimOutcome {
                status: AllocatorMaintenanceStatus::Unsupported,
                reason: AllocatorMaintenanceReason::NotSupported,
                elapsed_us: 0,
            }
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

/// Return the pages mimalloc is holding for us to the OS.
///
/// The global allocator is mimalloc on every 64-bit build with the default
/// `alloc-mimalloc` feature, and neither glibc's `malloc_trim` nor Darwin's
/// `malloc_zone_pressure_relief` reaches memory mimalloc owns: they trim the
/// system allocator, which holds almost nothing of ours. Every 1 MiB arena
/// block the sweep just deallocated therefore sat in mimalloc's segment cache
/// waiting for its delayed purge — a purge mimalloc only performs when the
/// thread next allocates or frees through it, which is exactly what an idle
/// program does not do. Measured on the idle-reclaim fixture before this call
/// existed: `heapUsed` 47 MB -> 18 MB with RSS unchanged at 115 MB.
///
/// `mi_collect(true)` is mimalloc's own trim: it frees deferred and
/// thread-delayed blocks, purges every free page of the calling thread's heap
/// and of abandoned segments back to the OS (decommit / `MADV_DONTNEED`, or
/// `MADV_FREE_REUSABLE` on Darwin, so the footprint accounting moves too), and
/// returns. Bounded allocator maintenance, same placement as the platform trim.
fn purge_mimalloc_cache() -> bool {
    #[cfg(all(target_pointer_width = "64", feature = "alloc-mimalloc"))]
    {
        // Declared here rather than through `libmimalloc_sys`: that crate is a
        // dependency on Apple targets only (it carries the Instruments memory
        // tag), while the `mimalloc` global allocator links the C library on
        // every target, so the symbol is always present under this feature.
        unsafe extern "C" {
            fn mi_collect(force: bool);
        }
        #[cfg(test)]
        TEST_MIMALLOC_PURGES.with(|c| c.set(c.get().saturating_add(1)));
        unsafe {
            mi_collect(true);
        }
        true
    }
    #[cfg(not(all(target_pointer_width = "64", feature = "alloc-mimalloc")))]
    {
        false
    }
}
