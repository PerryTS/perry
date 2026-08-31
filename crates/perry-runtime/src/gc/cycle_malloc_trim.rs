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

    #[cfg(target_env = "gnu")]
    {
        #[cfg(test)]
        record_test_malloc_trim_executed();

        let start = Instant::now();
        unsafe {
            libc::malloc_trim(0);
        }
        return MallocTrimOutcome {
            status: AllocatorMaintenanceStatus::Executed,
            reason: AllocatorMaintenanceReason::ExplicitOrEmergency,
            elapsed_us: start.elapsed().as_micros() as u64,
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
        let start = Instant::now();
        unsafe {
            // NULL zone = all zones; goal 0 = release as much as possible.
            malloc_zone_pressure_relief(core::ptr::null_mut(), 0);
        }
        return MallocTrimOutcome {
            status: AllocatorMaintenanceStatus::Executed,
            reason: AllocatorMaintenanceReason::ExplicitOrEmergency,
            elapsed_us: start.elapsed().as_micros() as u64,
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
