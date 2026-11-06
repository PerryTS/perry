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

/// Dynamic code is made: `kind` 0 = a `new Function` body, 1 = an eval
/// script. Counted per init (design §2.6: these cannot be image cells
/// until build-time compilation); `PERRY_SNAP_DYN=defer` makes them defer
/// the init (option (a), host write by kind).
#[inline(always)]
pub(crate) fn dyn_code(kind: u8) {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::dyn_code(kind);
    }
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    let _ = kind;
}

/// The census runner is on and has not reached its boundary: collection
/// stays suppressed (see `gc::gc_unsuppress`).
#[inline(always)]
pub(crate) fn holds_collection() -> bool {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    return runner::holds_collection();
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    false
}

/// `object_meta_ensure` is about to store a fresh, all-zero record into the
/// owner's meta word `slot` (design §2.10.11: an empty install is
/// representation). Instrumented builds only.
#[cfg(perry_gc_instruments)]
#[inline(always)]
pub(crate) fn meta_install(slot: usize) {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::meta_install(slot);
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    let _ = slot;
}

/// The record `meta` now hangs off the owner's meta word `slot`: its words
/// are the owner's. Instrumented builds only.
#[cfg(perry_gc_instruments)]
#[inline(always)]
pub(crate) fn meta_installed(slot: usize, meta: usize) {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::meta_installed(slot, meta);
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    let _ = (slot, meta);
}

/// A classification mark (`ensure_meta_for_mark`) runs while this lives:
/// representation, not program state (design §2.10.11). Instrumented
/// builds only.
#[cfg(perry_gc_instruments)]
#[inline(always)]
pub(crate) fn representation() -> Representation {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    if runner::on() && runner::representation(1) {
        return Representation { active: true };
    }
    Representation { active: false }
}

/// The scope [`representation`] opened: its end closes the representation
/// depth with the bookkeeping depth (R1 tells the two apart).
#[cfg(perry_gc_instruments)]
pub(crate) struct Representation {
    active: bool,
}

#[cfg(perry_gc_instruments)]
impl Drop for Representation {
    #[inline(always)]
    fn drop(&mut self) {
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        if self.active {
            runner::representation(-1);
        }
        #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
        let _ = self.active;
    }
}

/// A keyed operation on `obj` (a global lookup of key X, the add or store
/// of key K) names `key` for the scope's extent: on the realm global, the
/// census takes the global's shape word as the location of that key
/// (design §2.10.13); any other object is untouched. Census builds only.
#[cfg(perry_gc_instruments)]
#[inline(always)]
pub(crate) fn global_key<'a>(obj: usize, key: impl FnOnce() -> Option<&'a [u8]>) -> GlobalKey {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        return GlobalKey {
            prev: runner::global_key(obj, key),
        };
    }
    let _ = (obj, key);
    GlobalKey { prev: None }
}

#[cfg(perry_gc_instruments)]
pub(crate) struct GlobalKey {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    prev: Option<(u64, [u8; runner::GKEY_NAME], u8)>,
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    prev: Option<()>,
}

#[cfg(perry_gc_instruments)]
impl Drop for GlobalKey {
    #[inline(always)]
    fn drop(&mut self) {
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        if let Some(prev) = self.prev.take() {
            runner::global_key_end(prev);
        }
        #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
        let _ = self.prev;
    }
}

/// `cells` (an object's metadata record, its overflow table) now belong to
/// `obj`: the cells of a transparent object are transparent too.
#[inline(always)]
pub(crate) fn attached(obj: usize, cells: &[usize]) {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::attached(obj, cells);
    }
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    let _ = (obj, cells);
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
#[cfg_attr(not(perry_gc_instruments), allow(dead_code))]
pub(crate) fn job_owner() -> u32 {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        return runner::current_init();
    }
    0
}

/// A job owned by `owner` (from [`job_owner`] at its enqueue) starts.
#[inline(always)]
#[cfg_attr(not(perry_gc_instruments), allow(dead_code))]
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

/// An explicit `gc()`: the boundary marker of a census run. True when the
/// collection must not run (`PERRY_SNAP_RSET` keeps the heap in place).
#[inline(always)]
pub(crate) fn explicit_gc() -> bool {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    {
        if runner::on() || runner::start_if_requested() {
            runner::boundary("gc");
        }
        return runner::skip_collection();
    }
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    false
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

/// A reused slot handed out at `user` (its GcHeader sits just below):
/// [`birth`] of the allocation. The header address is computed only in
/// instrumented builds, so a production caller computes nothing new.
#[inline(always)]
pub(crate) fn birth_user(user: usize, size: usize) {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::birth(user - crate::gc::GC_HEADER_SIZE, size);
    }
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    let _ = (user, size);
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

/// Runtime work that an init triggers but does not own (populating the
/// realm on first use): its births, accesses and host touches are the
/// runtime's (init 0), reported apart.
#[inline(always)]
pub(crate) fn runtime_scope() -> RuntimeScope {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::job_enter(0);
        return RuntimeScope { active: true };
    }
    RuntimeScope { active: false }
}

/// The fill of a cache of static data on its first use (a literal class's
/// keys, a regex literal site's matcher data): the same whichever init asks
/// first, so the census counts it as the runtime's, like [`runtime_scope`]
/// (design §2.10.20; `PERRY_SNAP_FIRSTUSE=0` leaves it the asking init's).
#[inline(always)]
pub(crate) fn first_use() -> RuntimeScope {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() && runner::first_use_on() {
        runner::job_enter(0);
        return RuntimeScope { active: true };
    }
    RuntimeScope { active: false }
}

/// The lazy birth and build of a realm intrinsic (the global object,
/// %Object%, a generator or iterator tower, %TypedArray%): the same
/// whichever init asks first, so the census counts it as the runtime's, like
/// [`runtime_scope`] (design §2.10.20; `PERRY_SNAP_REALM=0` leaves it the
/// asking init's). Outside census builds the scope is a unit value with no
/// drop, so the builders compile exactly as without it.
#[inline(always)]
pub(crate) fn realm_build() -> RealmScope {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() && runner::realm_on() {
        runner::job_enter(0);
        return RealmScope { active: true };
    }
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    return RealmScope { active: false };
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    RealmScope {}
}

/// The scope of [`realm_build`].
pub(crate) struct RealmScope {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    active: bool,
}

#[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
impl Drop for RealmScope {
    #[inline(always)]
    fn drop(&mut self) {
        if self.active {
            runner::job_exit();
        }
    }
}

pub(crate) struct RuntimeScope {
    active: bool,
}

impl Drop for RuntimeScope {
    #[inline(always)]
    fn drop(&mut self) {
        #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
        if self.active {
            runner::job_exit();
        }
        #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
        let _ = self.active;
    }
}

/// Codegen calls this where the entry module's own body starts (after its
/// eager imports), in instrumented compiles only (`PERRY_SNAP_CENSUS` set
/// while compiling). The census boundary of an ESM entry.
/// The program evaluates its entry as a module (it has a worker): the
/// evaluation that starts next is the entry body (design §2.3). Emitted by
/// census compiles only, right before the entry module's `__init`.
#[cfg(perry_gc_instruments)]
#[no_mangle]
pub extern "C" fn js_snap_entry_module() {
    crate::effect!(pure);
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() || runner::start_if_requested() {
        runner::entry_module();
    }
}

/// The program's TLS slot ranges: census compiles call this first in `main`
/// with the addresses of the four page-aligned anchors the driver links
/// before and after the program objects (`.tdata` and `.tbss` each). Only
/// these pages are protected; runtime thread-locals stay outside them.
#[cfg(perry_gc_instruments)]
#[no_mangle]
pub extern "C" fn js_snap_tls_program(
    data_begin: *const u8,
    data_end: *const u8,
    bss_begin: *const u8,
    bss_end: *const u8,
) {
    crate::effect!(pure);
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    runner::tls_program(
        data_begin as usize,
        data_end as usize,
        bss_begin as usize,
        bss_end as usize,
    );
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    let _ = (data_begin, data_end, bss_begin, bss_end);
}

#[cfg(perry_gc_instruments)]
#[no_mangle]
pub extern "C" fn js_snap_entry_body() {
    crate::effect!(pure);
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() || runner::start_if_requested() {
        runner::entry_body();
    }
}

// ---------------------------------------------------------------------------
// SNAP3: env reads as startup-filled cells and one image guard (design
// §2.10). The three env funnels call these instead of declaring
// `host_read env`; an init that reads the env through them keeps its image
// and the runner records a cell or a guard member.
// ---------------------------------------------------------------------------

/// The env object (`process.env` as a value) was handed to the program:
/// `boxed` is its NaN-boxed bits. Taking it is not an observation; its words
/// in the image are `env_object` cells.
#[inline(always)]
#[cfg_attr(not(perry_gc_instruments), allow(dead_code))]
pub(crate) fn env_object(boxed: f64) {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::env_object(boxed.to_bits());
    }
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    let _ = boxed;
}

/// The env object's storage is about to be populated (the birth-log mark for
/// [`env_storage_end`]).
#[inline(always)]
pub(crate) fn env_storage_begin() -> usize {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        return runner::births_len();
    }
    0
}

/// The env object `obj` was populated: what was born since `begin` (its key
/// and value strings, key lists, overflow) and the object's slots are the
/// env block's words. An init that reads or writes one enumerates or writes
/// the env (rule 1).
#[inline(always)]
pub(crate) fn env_storage_end(begin: usize, obj: usize) {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::env_storage(begin, obj);
    }
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    let _ = (begin, obj);
}

/// One var read by name (`js_getenv`): `value` is its text and `ptr` the
/// string `js_getenv` returned (0 when the var is unset).
#[inline(always)]
pub(crate) fn env_var(name: &str, value: Option<&str>, ptr: usize) {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::env_var(name, value, ptr);
    }
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    let _ = (name, value, ptr);
}

/// A presence test of one var (`"X" in env`, `hasOwnProperty`, `Reflect.has`).
#[inline(always)]
#[cfg_attr(not(perry_gc_instruments), allow(dead_code))]
pub(crate) fn env_has(name: &str, present: bool) {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::env_has(name, present);
    }
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    let _ = (name, present);
}

/// Codegen calls this right before a `process.env.X` / `process.env[k]`
/// read, in instrumented compiles only (`expr/snap_env_site.rs`): `kind` 1
/// when the read's value is stored straight into a module global, a
/// property, an element or a literal; 0 otherwise. Kept for the frame's
/// next `js_getenv`.
#[cfg(perry_gc_instruments)]
#[no_mangle]
pub extern "C" fn js_snap_env_site(kind: i32) {
    crate::effect!(pure);
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::env_site(kind);
    }
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    let _ = kind;
}

/// Codegen calls this around a literal CommonJS require edge's init-guard
/// call, in instrumented compiles only (`cjsRequireEdge`): `on` 1 before, 0
/// after. The evaluation the guard starts was reached by `require` (a static
/// edge), not by `import()`: it is no member of a dynamic subgraph (design
/// §2.10.7). A missing mark costs precision, never soundness.
#[cfg(perry_gc_instruments)]
#[no_mangle]
pub extern "C" fn js_snap_require_edge(on: i32) {
    crate::effect!(pure);
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::require_edge(on != 0);
    }
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    let _ = on;
}

/// The check run (`PERRY_SNAP_SKIP=<D file>`): true when the evaluation now
/// running is a member of D, whose body parts are skipped.
#[inline(always)]
pub(crate) fn skip_body() -> bool {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        return runner::skip_body();
    }
    false
}

/// The evaluation's emitted once guard, retained on its census init record.
#[cfg(perry_gc_instruments)]
#[inline(always)]
pub(crate) fn init_guard(addr: usize, guard: usize) {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    if runner::on() {
        runner::init_guard(addr, guard);
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    let _ = (addr, guard);
}
