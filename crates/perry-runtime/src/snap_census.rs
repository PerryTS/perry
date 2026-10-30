//! SNAP phase 1: what a build-time snapshot of module initialization would
//! image and what it would defer (`secret-tests/nightly/CW/SNAP_DESIGN_2026-10-30.md`).
//!
//! Two parts live here:
//!
//! - **Effect classes.** A runtime entry that observes or changes anything
//!   outside the JS heap says so at its definition with [`effect!`]:
//!   `target_fact` (a constant of the compile target), `host_read` (a value of
//!   the machine or the invocation) or `host_write` (changes the process or
//!   the world). An entry that reads and writes only the JS heap says
//!   `effect!(pure)`. The declaration is read twice: by the census runner at
//!   run time (the macro expands to a call of [`host`]), and from the source
//!   by the effect-class gate, which checks that every runtime entry codegen
//!   can call carries one (an entry without a class counts as host write).
//!   In the default archives [`host`] is an empty inline function, so the
//!   declaration compiles to nothing.
//!
//! - **The census runner** (`PERRY_SNAP_CENSUS=<out>`, instrumented runtime
//!   only): runs the program's module initialization up to the boundary
//!   (the entry body's start, or the first explicit `gc()`), with collection
//!   suppressed so an address names one object for the whole run, and records
//!   per init: its extent, how it was reached, its exclusive instruction
//!   count, its births, its host touches, and (with page protection over the
//!   GC reservation, the realm image and the module-global root slots) the
//!   exact words the deferred inits read and wrote and the first access of
//!   any other init that conflicts with them (design §2.3 rules 2-4). See
//!   `snap_census/runner.rs`.
//!
//! Every hook below is `#[inline(always)]` and empty without the
//! `perry_gc_instruments` cfg (and off Linux x86_64, where the runner's page
//! protection and single-step do not exist); with it, a hook that finds the
//! runner off costs one relaxed load.

/// The effect class of a runtime entry (design §2.2). `pure` has no value:
/// it is the absence of an effect.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Effect {
    /// A constant of the compile target (`process.platform`, `os.EOL`):
    /// the snapshot answers the target's value, the init stays imageable.
    TargetFact = 1,
    /// A value of the machine or the invocation (env, argv, cwd, tty, ids,
    /// default locale and time zone, clocks, randomness, fs reads): the
    /// reading init is deferred.
    HostRead = 2,
    /// A change of the process or the world (stdio, env writes, listeners,
    /// timers and ticks, exit code, fs/net/child process, workers): the
    /// writing init is deferred.
    HostWrite = 3,
}

/// Declare the effect class of the runtime entry this statement opens.
///
/// `effect!(pure)`, `effect!(target_fact, <what>)`, `effect!(host_read,
/// <what>)`, `effect!(host_write, <what>)`; `<what>` names the host state
/// (`env`, `argv`, `tty`, `clock`, ...) for the census report. The gate reads
/// this form from the source, so it is one statement at the top of the entry
/// body, never computed.
#[macro_export]
macro_rules! effect {
    (pure) => {};
    (target_fact, $what:ident) => {
        $crate::snap_census::host($crate::snap_census::Effect::TargetFact, stringify!($what))
    };
    (host_read, $what:ident) => {
        $crate::snap_census::host($crate::snap_census::Effect::HostRead, stringify!($what))
    };
    (host_write, $what:ident) => {
        $crate::snap_census::host($crate::snap_census::Effect::HostWrite, stringify!($what))
    };
}

#[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
#[path = "snap_census/runner.rs"]
mod runner;

/// The entry that declared `class` is running (see [`effect!`]).
#[inline(always)]
pub fn host(class: Effect, what: &'static str) {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::host(class, what);
    }
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    let _ = (class, what);
}

/// A module evaluation (`run_path_initializer`) starts: `addr` is its
/// evaluation body (a guarded module's `<prefix>__init_evaluate`, a path
/// module's initializer).
#[inline(always)]
pub(crate) fn init_enter(addr: usize) {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    runner::init_enter(addr);
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    let _ = addr;
}

/// The evaluation [`init_enter`] opened returned; `threw` when it threw.
#[inline(always)]
pub(crate) fn init_exit(threw: bool) {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::init_exit(threw);
    }
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    let _ = threw;
}

/// `js_run_module_init_catching` runs one part of the current evaluation
/// (its literal preparation, then its body).
#[inline(always)]
pub(crate) fn init_part() {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::init_part();
    }
}

/// A `require` of a path module is in progress (the next evaluation it
/// starts was reached by `require`, not by an import).
#[inline(always)]
pub(crate) fn require_scope() -> RequireScope {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::require_depth(1);
        return RequireScope { active: true };
    }
    RequireScope { active: false }
}

pub(crate) struct RequireScope {
    active: bool,
}

impl Drop for RequireScope {
    #[inline(always)]
    fn drop(&mut self) {
        #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
        if self.active {
            runner::require_depth(-1);
        }
        #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
        let _ = self.active;
    }
}

/// The init a job enqueued now belongs to (0: none).
#[inline(always)]
pub(crate) fn job_owner() -> u32 {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        return runner::current_init();
    }
    0
}

/// A job owned by `owner` (from [`job_owner`] at its enqueue) starts.
#[inline(always)]
pub(crate) fn job_enter(owner: u32) {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::job_enter(owner);
    }
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    let _ = owner;
}

/// The job [`job_enter`] opened is over.
#[inline(always)]
pub(crate) fn job_exit() {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::job_exit();
    }
}

/// An explicit `gc()`: the boundary marker of a census run.
#[inline(always)]
pub(crate) fn explicit_gc() {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::boundary("gc");
    }
}

/// The census of the collection the boundary's `gc()` runs has finished.
#[inline(always)]
pub(crate) fn explicit_gc_done() {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    runner::after_boundary_collection();
}

/// A walkable object is live at the boundary collection's census.
#[inline(always)]
pub(crate) fn census_live(header: *const u8, obj_type: u8, size: usize) {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    runner::census_live(header, obj_type, size);
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    let _ = (header, obj_type, size);
}

/// An arena bump handed out `[ptr, ptr + size)`.
#[inline(always)]
pub(crate) fn birth(ptr: *mut u8, size: usize) {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::birth(ptr as usize, size);
    }
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    let _ = (ptr, size);
}

/// While the census runs, compiled code must allocate through the runtime
/// (every birth is logged), so the inline bump bound is closed.
#[inline(always)]
pub(crate) fn inline_bump_closed() -> bool {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    return runner::on();
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    false
}

/// The reservation mapped `[base, base + len)` (a new arena block).
#[inline(always)]
pub(crate) fn region_mapped(base: usize, len: usize) {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::region_mapped(base, len);
    }
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    let _ = (base, len);
}

/// A module-global root slot was registered.
#[inline(always)]
pub(crate) fn global_root(slot: usize) {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::global_root(slot);
    }
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    let _ = slot;
}

/// Runtime bookkeeping that reads JS state without the program observing it
/// (root registration verifies the word it registers): its accesses are not
/// the current init's.
#[inline(always)]
pub(crate) fn bookkeeping() -> Bookkeeping {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::bookkeeping_depth(1);
        return Bookkeeping { active: true };
    }
    Bookkeeping { active: false }
}

pub(crate) struct Bookkeeping {
    active: bool,
}

impl Drop for Bookkeeping {
    #[inline(always)]
    fn drop(&mut self) {
        #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
        if self.active {
            runner::bookkeeping_depth(-1);
        }
        #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
        let _ = self.active;
    }
}

/// Codegen calls this where the entry module's own body starts (after its
/// eager imports), in instrumented compiles only (`PERRY_SNAP_CENSUS` set
/// while compiling). The census boundary of an ESM entry.
#[no_mangle]
pub extern "C" fn js_snap_entry_body() {
    crate::effect!(pure);
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::entry_body();
    }
}
