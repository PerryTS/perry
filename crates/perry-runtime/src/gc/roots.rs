use super::*;
use std::any::Any;

mod runtime_handles;
mod scanner_shims;
mod shadow_stack;

pub(super) use runtime_handles::{
    new_runtime_handle_root_scan_state, scan_runtime_handle_roots_mut,
    scan_runtime_handle_roots_mut_step,
};
pub use runtime_handles::{RuntimeHandle, RuntimeHandleScope};
pub use scanner_shims::{
    async_context_mutable_root_scanner, async_context_root_scanner,
    async_hooks_mutable_root_scanner, async_hooks_root_scanner, exception_mutable_root_scanner,
    exception_root_scanner, intern_table_mutable_root_scanner, intern_table_root_scanner,
    json_parse_mutable_root_scanner, json_parse_root_scanner, overflow_fields_mutable_root_scanner,
    overflow_fields_root_scanner, promise_mutable_root_scanner, promise_root_scanner,
    shadow_stack_root_scanner, shape_cache_mutable_root_scanner, shape_cache_root_scanner,
    small_int_cache_mutable_root_scanner, small_int_cache_root_scanner, timer_mutable_root_scanner,
    timer_root_scanner, transition_cache_mutable_root_scanner, transition_cache_root_scanner,
};
pub(crate) use shadow_stack::shadow_stack_has_active_frame;
pub(crate) use shadow_stack::SHADOW;
pub use shadow_stack::{
    js_shadow_frame_pop, js_shadow_frame_push, js_shadow_slot_bind, js_shadow_slot_get,
    js_shadow_slot_set, shadow_stack_depth, SHADOW_STACK_GROW_RESERVE, SHADOW_STACK_HEADER_SLOTS,
};
pub(crate) use shadow_stack::{shadow_stack_restore, shadow_stack_savepoint, ShadowSavepoint};

pub type MutableRootScanner = for<'a> fn(&mut RuntimeRootVisitor<'a>);
pub(crate) type BudgetedMutableRootScanner =
    for<'a> fn(&mut RuntimeRootVisitor<'a>, &mut dyn Any, &mut usize) -> bool;
pub(crate) type BudgetedMutableRootScannerStateFactory = fn() -> Box<dyn Any>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MutableRootScannerSource {
    RuntimeHandles,
    RuntimeMutableScanner,
}

#[derive(Clone, Copy)]
pub(super) struct MutableRootScannerEntry {
    pub(super) scanner: MutableRootScanner,
    pub(super) source: MutableRootScannerSource,
    pub(super) budgeted_scanner: Option<BudgetedMutableRootScanner>,
    pub(super) budgeted_state_factory: Option<BudgetedMutableRootScannerStateFactory>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RuntimeHandleSlot {
    Nanbox(u64),
    RawTagged { addr: usize, tag: u64 },
    HeapWord(u64),
}

thread_local! {
    pub(super) static ROOT_SCANNERS: RefCell<Vec<fn(&mut dyn FnMut(f64))>> = RefCell::new(Vec::new());
    pub(super) static MUTABLE_ROOT_SCANNERS: RefCell<Vec<MutableRootScannerEntry>> = const { RefCell::new(Vec::new()) };
    pub(super) static FFI_ROOT_SCANNERS: RefCell<Vec<PerryFfiRootScanner>> = RefCell::new(Vec::new());
    pub(super) static FFI_MUTABLE_ROOT_SCANNERS: RefCell<Vec<PerryFfiMutableRootScanner>> = RefCell::new(Vec::new());
    pub(super) static FFI_NAMED_MUTABLE_ROOT_SCANNERS: RefCell<Vec<(PerryFfiNamedMutableRootScanner, usize)>> = RefCell::new(Vec::new());
    pub(super) static GLOBAL_ROOTS: RefCell<Vec<*mut u64>> = const { RefCell::new(Vec::new()) };
    pub(super) static RUNTIME_HANDLE_STACK: RefCell<Vec<RuntimeHandleSlot>> = const { RefCell::new(Vec::new()) };
    pub(super) static GC_ROOT_LOCK_DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// Guard returned by `lock_gc_root_registry`.
///
/// The mutex is released before any deferred GC request is flushed. That
/// drop order is what lets scanner-owned registries use ordinary blocking
/// locks in their root scanners: a GC request made while the same mutex is
/// held records pending work, returns immediately, and the final guard drop
/// runs the collection only after the scanner can reacquire the mutex.
pub(crate) struct GcRootRegistryGuard<'a, T> {
    pub(super) guard: Option<MutexGuard<'a, T>>,
}

impl<T> std::ops::Deref for GcRootRegistryGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.guard
            .as_deref()
            .expect("GC root registry guard missing")
    }
}

impl<T> std::ops::DerefMut for GcRootRegistryGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard
            .as_deref_mut()
            .expect("GC root registry guard missing")
    }
}

impl<T> Drop for GcRootRegistryGuard<'_, T> {
    fn drop(&mut self) {
        drop(self.guard.take());
        exit_gc_root_lock();
    }
}

pub(crate) fn lock_gc_root_registry<T>(mutex: &Mutex<T>) -> GcRootRegistryGuard<'_, T> {
    let guard = mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    enter_gc_root_lock();
    GcRootRegistryGuard { guard: Some(guard) }
}

#[inline]
pub(super) fn enter_gc_root_lock() {
    GC_ROOT_LOCK_DEPTH.with(|depth| depth.set(depth.get() + 1));
}

pub(super) fn exit_gc_root_lock() {
    let should_flush = GC_ROOT_LOCK_DEPTH.with(|depth| {
        let current = depth.get();
        debug_assert!(current > 0, "GC root lock depth underflow");
        if current == 0 {
            return false;
        }
        depth.set(current - 1);
        current == 1
    });
    if should_flush {
        flush_deferred_gc_request();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConservativeStackScanMode {
    Auto,
    Disabled,
    Full,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub(super) enum ConservativeStackScanDecision {
    Scan,
    #[default]
    SkipDisabled,
}

impl ConservativeStackScanDecision {
    #[cfg(feature = "diagnostics")]
    #[inline]
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Scan => "scan",
            Self::SkipDisabled => "skip_disabled",
        }
    }
}

pub(super) fn conservative_stack_scan_mode_from_value(
    value: Option<&str>,
) -> ConservativeStackScanMode {
    let Some(value) = value else {
        return ConservativeStackScanMode::Auto;
    };
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "auto" => ConservativeStackScanMode::Auto,
        "0" | "off" | "false" => ConservativeStackScanMode::Disabled,
        "1" | "on" | "true" | "full" | "debug" => ConservativeStackScanMode::Full,
        _ => ConservativeStackScanMode::Auto,
    }
}

thread_local! {
    /// Per-thread override for the conservative-scan mode, taking precedence over
    /// the `#[cfg(test)]` default but not an explicit env var. The GC unit tests
    /// set this to `Auto` (skip) inside their controlled-root scopes so a forced
    /// collection still reclaims the objects they hold only as native-stack
    /// locals; see `ScopedRootScannerRegistryGuard`.
    pub(super) static CONSERVATIVE_STACK_SCAN_OVERRIDE: std::cell::Cell<Option<ConservativeStackScanMode>> =
        const { std::cell::Cell::new(None) };
}

/// Set (or clear) this thread's conservative-scan mode override, returning the
/// previous value.
pub(crate) fn set_conservative_stack_scan_override(
    mode: Option<ConservativeStackScanMode>,
) -> Option<ConservativeStackScanMode> {
    CONSERVATIVE_STACK_SCAN_OVERRIDE.with(|c| c.replace(mode))
}

/// Scoped guard forcing the conservative native-stack scan for an explicit
/// `gc()` collection (#4977). In the default `Auto` mode a full collection
/// skips the native scan, but at a `gc()` callsite live module-init/top-level
/// locals may be held only on the native stack — neither the precise
/// shadow-stack roots nor the module-var scanners cover them — so the
/// collector reclaimed live object graphs and later field reads returned
/// dangling-pointer garbage. An already-pinned per-thread override wins (the
/// GC unit tests pin `Auto` so a forced collection still reclaims objects they
/// hold only as native-stack locals), and an explicit
/// `PERRY_CONSERVATIVE_STACK_SCAN` env value beats any override either way,
/// so the bisection escape hatch keeps working.
pub(super) struct ManualGcScanGuard {
    engaged: bool,
}

impl ManualGcScanGuard {
    pub(super) fn force_full_scan() -> Self {
        let engaged = CONSERVATIVE_STACK_SCAN_OVERRIDE.with(|c| {
            if c.get().is_some() {
                return false;
            }
            c.set(Some(ConservativeStackScanMode::Full));
            true
        });
        Self { engaged }
    }
}

impl Drop for ManualGcScanGuard {
    fn drop(&mut self) {
        if self.engaged {
            CONSERVATIVE_STACK_SCAN_OVERRIDE.with(|c| c.set(None));
        }
    }
}

pub(super) fn conservative_stack_scan_mode() -> ConservativeStackScanMode {
    // Explicit env var wins (so the dedicated mode tests and ops bisection keep
    // working), then a per-thread override, then the build-time default.
    if let Ok(value) = std::env::var("PERRY_CONSERVATIVE_STACK_SCAN") {
        return conservative_stack_scan_mode_from_value(Some(&value));
    }
    if let Some(mode) = CONSERVATIVE_STACK_SCAN_OVERRIDE.with(|c| c.get()) {
        return mode;
    }
    // Unit tests root GC-managed pointers as raw locals on the *native* (Rust)
    // stack, not the shadow stack, so without the conservative native scan any
    // collection triggered mid-test (e.g. by an allocation in a helper) reclaims
    // them and a later header deref faults. Default the test build to a full
    // scan so those locals survive; production keeps live values on the shadow
    // stack and skips the imprecise, costly native scan.
    #[cfg(test)]
    {
        ConservativeStackScanMode::Full
    }
    #[cfg(not(test))]
    {
        ConservativeStackScanMode::Auto
    }
}

#[inline]
pub(super) fn conservative_stack_scan_decision_for(
    mode: ConservativeStackScanMode,
    _shadow_frame_active: bool,
) -> ConservativeStackScanDecision {
    match mode {
        ConservativeStackScanMode::Disabled => ConservativeStackScanDecision::SkipDisabled,
        ConservativeStackScanMode::Full => ConservativeStackScanDecision::Scan,
        ConservativeStackScanMode::Auto => ConservativeStackScanDecision::SkipDisabled,
    }
}

pub(super) fn conservative_stack_scan_decision() -> ConservativeStackScanDecision {
    conservative_stack_scan_decision_for(
        conservative_stack_scan_mode(),
        shadow_stack_has_active_frame(),
    )
}

/// Register a root scanner function.
/// Each scanner is called during the mark phase to discover roots.
/// This legacy API exposes copied values only. It remains supported for
/// fallback/full GC, where discovered targets can be pinned, but registering
/// any copy-only scanner makes low-pause copied-minor collection ineligible.
pub fn gc_register_root_scanner(scanner: fn(&mut dyn FnMut(f64))) {
    ROOT_SCANNERS.with(|scanners| {
        scanners.borrow_mut().push(scanner);
    });
}

pub(super) fn copy_only_root_scanner_counts() -> (usize, usize) {
    let rust_scanners = ROOT_SCANNERS.with(|scanners| scanners.borrow().len());
    let ffi_scanners = FFI_ROOT_SCANNERS.with(|scanners| scanners.borrow().len());
    (rust_scanners, ffi_scanners)
}

pub(super) fn registered_root_scanners_block_budgeted_gc() -> bool {
    let has_copy_only = ROOT_SCANNERS.with(|scanners| !scanners.borrow().is_empty())
        || FFI_ROOT_SCANNERS.with(|scanners| !scanners.borrow().is_empty());
    if super::gc_incremental_enabled() {
        // Phase 4 (incremental old-gen, gated): unbudgeted MUTABLE / ffi-mutable
        // scanners no longer block the cycle — they run synchronously in the
        // initial root-scan step (see `allow_synchronous_scanners`), a bounded
        // initial-mark pause. Copy-only scanners still block: they can only
        // mark, not rewrite, so a moving/incremental cycle can't tolerate them.
        // (There are ~none in production — only the FFI copy-only API + tests.)
        return has_copy_only;
    }
    let has_unbudgeted_mutable = MUTABLE_ROOT_SCANNERS.with(|scanners| {
        scanners
            .borrow()
            .iter()
            .any(|entry| entry.budgeted_scanner.is_none())
    });
    let has_ffi_mutable = FFI_MUTABLE_ROOT_SCANNERS.with(|scanners| !scanners.borrow().is_empty())
        || FFI_NAMED_MUTABLE_ROOT_SCANNERS.with(|scanners| !scanners.borrow().is_empty());
    has_copy_only || has_unbudgeted_mutable || has_ffi_mutable
}

/// Register a runtime-owned root scanner that exposes mutable slots.
/// These scanners are marked like ordinary roots, but their storage is
/// revisited after evacuation so forwarded references can be rewritten.
///
/// This compatibility registration has no resumable cursor. Ordinary
/// low-pause GC steps therefore treat it as a synchronous-only scanner and
/// defer while it is registered. Runtime scanners that can participate in
/// `NormalIncremental`/`MutatorAssist` progress must use
/// `gc_register_budgeted_mutable_root_scanner_with_source`.
pub fn gc_register_mutable_root_scanner(scanner: MutableRootScanner) {
    gc_register_mutable_root_scanner_with_source(
        scanner,
        MutableRootScannerSource::RuntimeMutableScanner,
    );
}

/// Compatibility wrapper for callers that provide a human-readable scanner
/// name. Current root-source telemetry groups these under runtime mutable
/// scanners.
pub fn gc_register_mutable_root_scanner_named(_source: &'static str, scanner: MutableRootScanner) {
    gc_register_mutable_root_scanner(scanner);
}

pub(super) fn gc_register_mutable_root_scanner_with_source(
    scanner: MutableRootScanner,
    source: MutableRootScannerSource,
) {
    MUTABLE_ROOT_SCANNERS.with(|scanners| {
        scanners.borrow_mut().push(MutableRootScannerEntry {
            scanner,
            source,
            budgeted_scanner: None,
            budgeted_state_factory: None,
        });
    });
}

pub(super) fn gc_register_budgeted_mutable_root_scanner_with_source(
    scanner: MutableRootScanner,
    budgeted_scanner: BudgetedMutableRootScanner,
    budgeted_state_factory: BudgetedMutableRootScannerStateFactory,
    source: MutableRootScannerSource,
) {
    MUTABLE_ROOT_SCANNERS.with(|scanners| {
        scanners.borrow_mut().push(MutableRootScannerEntry {
            scanner,
            source,
            budgeted_scanner: Some(budgeted_scanner),
            budgeted_state_factory: Some(budgeted_state_factory),
        });
    });
}

pub(super) type PerryFfiRootMarker = extern "C" fn(value: f64, ctx: *mut c_void);
pub(super) type PerryFfiRootScanner = extern "C" fn(mark: PerryFfiRootMarker, ctx: *mut c_void);
pub(super) type PerryFfiMutableRootVisitor =
    extern "C" fn(kind: u32, slot: *mut c_void, ctx: *mut c_void) -> bool;
pub(super) type PerryFfiMutableRootScanner =
    extern "C" fn(visit: PerryFfiMutableRootVisitor, ctx: *mut c_void);
pub(super) type PerryFfiNamedMutableRootScanner =
    extern "C" fn(scanner_id: usize, visit: PerryFfiMutableRootVisitor, ctx: *mut c_void);

pub(super) const PERRY_FFI_ROOT_SLOT_I64: u32 = 1;
pub(super) const PERRY_FFI_ROOT_SLOT_USIZE: u32 = 2;
pub(super) const PERRY_FFI_ROOT_SLOT_RAW_MUT_PTR: u32 = 3;
pub(super) const PERRY_FFI_ROOT_SLOT_NANBOX_F64: u32 = 4;
pub(super) const PERRY_FFI_ROOT_SLOT_NANBOX_U64: u32 = 5;

/// Register a native-package root scanner through a stable C ABI.
///
/// `perry-ffi` adapts its Rust-facing `fn(&mut dyn FnMut(f64))`
/// convenience API to this callback shape so native wrapper archives
/// can stay runtime-free. Like the Rust legacy scanner API, this is
/// copy-only storage from the GC's perspective. Registration keeps the
/// fallback/full-GC path compatible, but makes low-pause copied-minor
/// collection ineligible because native-owned slots cannot be rewritten.
#[no_mangle]
pub extern "C" fn perry_ffi_gc_register_root_scanner(scanner: PerryFfiRootScanner) {
    FFI_ROOT_SCANNERS.with(|scanners| {
        scanners.borrow_mut().push(scanner);
    });
}

/// Register a native-package root scanner that exposes mutable slots
/// through the stable C ABI.
///
/// Unlike `perry_ffi_gc_register_root_scanner`, this scanner can be
/// revisited after copied-minor evacuation so native-owned slots are
/// rewritten to forwarded addresses instead of forcing copy-only
/// pinning/fallback behavior.
#[no_mangle]
pub extern "C" fn perry_ffi_gc_register_mutable_root_scanner(scanner: PerryFfiMutableRootScanner) {
    FFI_MUTABLE_ROOT_SCANNERS.with(|scanners| {
        scanners.borrow_mut().push(scanner);
    });
}

/// Register a native-package mutable scanner with an associated wrapper id.
///
/// The `source` bytes are accepted for ABI compatibility with `perry-ffi`;
/// current root-source telemetry groups these with other FFI mutable scanners.
#[no_mangle]
pub extern "C" fn perry_ffi_gc_register_mutable_root_scanner_named(
    _source_ptr: *const u8,
    _source_len: usize,
    scanner_id: usize,
    scanner: PerryFfiNamedMutableRootScanner,
) {
    FFI_NAMED_MUTABLE_ROOT_SCANNERS.with(|scanners| {
        scanners.borrow_mut().push((scanner, scanner_id));
    });
}

/// Register a global variable address as a GC root.
/// Called by codegen in module init functions.
#[no_mangle]
pub extern "C" fn js_gc_register_global_root(ptr: i64) {
    let root = ptr as *mut u64;
    if !root.is_null() {
        unsafe {
            runtime_write_barrier_root_heap_word(*root);
        }
    }
    GLOBAL_ROOTS.with(|roots| {
        roots.borrow_mut().push(root);
    });
}

/// Suppress GC triggers. While suppressed, `gc_check_trigger` is a no-op.

pub(super) fn mark_stack_roots_for_decision(
    valid_ptrs: &ValidPointerSet,
    decision: ConservativeStackScanDecision,
    pin_only_old: bool,
) -> ConservativeRootTraceStats {
    match decision {
        ConservativeStackScanDecision::Scan => mark_stack_roots_unchecked(valid_ptrs, pin_only_old),
        ConservativeStackScanDecision::SkipDisabled => ConservativeRootTraceStats::default(),
    }
}

/// Conservative stack scan: scan the current thread's stack for heap pointers.
/// Handles BOTH NaN-boxed pointers (POINTER_TAG/STRING_TAG/BIGINT_TAG) AND raw I64 pointers.
/// Raw I64 pointers arise from Perry's `is_array`/`is_string`/`is_pointer`/`is_closure` local
/// variables — codegen stores these as raw I64 words (not NaN-boxed) in registers and on stack.
pub(super) fn mark_stack_roots_unchecked(
    valid_ptrs: &ValidPointerSet,
    pin_only_old: bool,
) -> ConservativeRootTraceStats {
    let mut stats = ConservativeRootTraceStats::default();
    // Capture callee-saved registers into a buffer via setjmp.
    //
    // On Apple platforms the C `setjmp(3)` saves the signal mask via a
    // `sigprocmask` system call, which dominates GC cost (~25 μs per
    // call on arm64). We only need register capture, not signal-state
    // save — switch to `_setjmp(3)` (linker symbol `__setjmp`) on
    // Apple targets. See the matching switch in
    // `promise.rs::js_promise_run_microtasks` for the full rationale.
    //
    // The `setjmp` extern lives in `crate::ffi::setjmp` so this and
    // `promise.rs` share one libc-matching declaration (issue #856).
    // We view the buffer as `u64` slots here because the goal of this
    // path is to scan register-sized words for potential NaN-boxed /
    // raw pointers; the cast to `*mut c_int` at the FFI boundary is
    // the inverse of the cast `promise.rs` does from its `*mut i32`
    // buffer.
    //
    // Size check: 32 * 8 = 256 bytes, which exceeds the darwin arm64
    // `jmp_buf` (48 * 4 = 192 bytes) and every other platform we
    // currently support — see `crate::ffi::setjmp::JMP_BUF_MIN_BYTES`.
    let mut jmp_buf = [0u64; 32]; // oversized for safety
    unsafe {
        crate::ffi::setjmp::setjmp(jmp_buf.as_mut_ptr() as *mut std::os::raw::c_int);
    }

    // Scan the register buffer (covers callee-saved regs: x19-x28 on AArch64, rbx/rbp/r12-r15 on x86_64)
    for &word in &jmp_buf {
        if try_mark_conservative_word(word, valid_ptrs, pin_only_old) {
            stats.root_count += 1;
        }
    }

    // Issue #73: setjmp only captures callee-saved registers. On
    // macOS ARM64 that's x19-x28 + d8-d15 — it misses d0-d7 and
    // d16-d31 (caller-saved FP regs where LLVM may be holding a
    // NaN-boxed pointer across the async poll loop's internal calls,
    // especially under heavy optimization). Capture them explicitly
    // via inline asm so any spilling LLVM hasn't performed is
    // irrelevant — we read the regs directly as they stand at GC
    // entry. A value in d0-d31 ANY of which happens to be a
    // NaN-boxed heap pointer gets marked here.
    #[cfg(target_arch = "aarch64")]
    unsafe {
        let mut fp_regs: [u64; 32] = [0; 32];
        std::arch::asm!(
            "str d0,  [{buf}, #0x00]",
            "str d1,  [{buf}, #0x08]",
            "str d2,  [{buf}, #0x10]",
            "str d3,  [{buf}, #0x18]",
            "str d4,  [{buf}, #0x20]",
            "str d5,  [{buf}, #0x28]",
            "str d6,  [{buf}, #0x30]",
            "str d7,  [{buf}, #0x38]",
            "str d8,  [{buf}, #0x40]",
            "str d9,  [{buf}, #0x48]",
            "str d10, [{buf}, #0x50]",
            "str d11, [{buf}, #0x58]",
            "str d12, [{buf}, #0x60]",
            "str d13, [{buf}, #0x68]",
            "str d14, [{buf}, #0x70]",
            "str d15, [{buf}, #0x78]",
            "str d16, [{buf}, #0x80]",
            "str d17, [{buf}, #0x88]",
            "str d18, [{buf}, #0x90]",
            "str d19, [{buf}, #0x98]",
            "str d20, [{buf}, #0xa0]",
            "str d21, [{buf}, #0xa8]",
            "str d22, [{buf}, #0xb0]",
            "str d23, [{buf}, #0xb8]",
            "str d24, [{buf}, #0xc0]",
            "str d25, [{buf}, #0xc8]",
            "str d26, [{buf}, #0xd0]",
            "str d27, [{buf}, #0xd8]",
            "str d28, [{buf}, #0xe0]",
            "str d29, [{buf}, #0xe8]",
            "str d30, [{buf}, #0xf0]",
            "str d31, [{buf}, #0xf8]",
            buf = in(reg) fp_regs.as_mut_ptr(),
            options(nostack, preserves_flags),
        );
        for &word in &fp_regs {
            if try_mark_conservative_word(word, valid_ptrs, pin_only_old) {
                stats.root_count += 1;
            }
        }
    }

    // Get stack bounds
    let stack_top: usize;
    #[cfg(target_arch = "aarch64")]
    unsafe {
        let sp: u64;
        std::arch::asm!("mov {}, sp", out(reg) sp);
        stack_top = sp as usize;
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        let sp: u64;
        std::arch::asm!("mov {}, rsp", out(reg) sp);
        stack_top = sp as usize;
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        // Fallback: skip stack scan on unsupported architectures
        return stats;
    }

    let stack_bottom = get_stack_bottom();
    if stack_bottom == 0 {
        return stats; // Can't determine stack bounds
    }

    // Walk the stack from current SP to stack bottom.
    // Each 8-byte word may be: NaN-boxed pointer, raw I64 heap pointer, return addr, or plain value.
    let mut addr = stack_top;
    while addr < stack_bottom {
        let word = unsafe { *(addr as *const u64) };
        if try_mark_conservative_word(word, valid_ptrs, pin_only_old) {
            stats.root_count += 1;
        }
        addr += 8;
    }
    stats
}

/// Mark a value if it is a heap pointer — either NaN-boxed OR a raw I64 pointer.
/// Returns true if newly marked.
/// This is used for conservative scanning where Perry stores raw I64 pointers (for is_string/
/// is_array/is_pointer/is_closure vars) alongside NaN-boxed F64 values.
#[inline]
/// Conservative-scan variant of `try_mark_value_or_raw` (#5029).
///
/// In a MINOR cycle, a conservative stack discovery that lives in the OLD
/// generation is retained WITHOUT tracing: minors never sweep the old
/// generation, so the mark is not needed for survival, and `CONS_PINNED`
/// already excludes the object from every evacuation candidate set. Tracing
/// it is actively unsound: a stale stack word (a dead frame slot from an
/// earlier loop iteration) can resurrect a DEAD old object whose slots still
/// hold pointers into long-swept nursery memory. Once fresh nursery blocks
/// are allocated over those freed ranges, the resurrected object's slots
/// alias live young objects; tracing / force-evacuating / rewriting through
/// them corrupts the heap, and the old-young-edge verifier reports the same
/// aliases as phantom missing remembered-set edges (the
/// gc_write_barrier_stress signature: missing_edges=7710 on a dead 256 KB
/// array backing). A LIVE old object loses nothing here: its real old→young
/// edges are dirty-page-covered by the write barriers, which is the only
/// mechanism a minor uses to find them anyway, and precise roots (shadow
/// stack, module vars, registered scanners) still mark and trace as before.
///
/// FULL collections (`pin_only_old == false`) keep the old behavior: the old
/// sweep frees unmarked old objects, so a stack-only-referenced old object
/// must be marked for retention there (#4977).
pub(super) fn try_mark_conservative_word(
    word: u64,
    valid_ptrs: &ValidPointerSet,
    pin_only_old: bool,
) -> bool {
    if !pin_only_old {
        return try_mark_value_or_raw(word, valid_ptrs);
    }

    // Resolve the candidate exactly like `try_mark_value_or_raw` —
    // NaN-boxed heap tags first, then the raw-I64 pointer fallback with the
    // interior-pointer (`enclosing_object`) lookup.
    let tag = word & TAG_MASK;
    let target = if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
        let ptr_val = (word & POINTER_MASK) as usize;
        if ptr_val == 0 || !valid_ptrs.maybe_contains(ptr_val) || !valid_ptrs.contains(&ptr_val) {
            return false;
        }
        ptr_val
    } else {
        if !(0x1000..=0x0000_FFFF_FFFF_FFFF).contains(&word) {
            return false;
        }
        let raw_ptr = word as usize;
        if !valid_ptrs.maybe_contains(raw_ptr)
            && raw_ptr.saturating_sub(0x10_0000) > valid_ptrs.range_max
        {
            return false;
        }
        if valid_ptrs.contains(&raw_ptr) {
            raw_ptr
        } else {
            match valid_ptrs.enclosing_object(raw_ptr) {
                Some(start) => start,
                None => return false,
            }
        }
    };

    unsafe {
        let header = header_from_user_ptr(target as *const u8);
        if (*header).gc_flags & GC_FLAG_MARKED != 0 {
            return false;
        }
        if (*header).gc_flags & GC_FLAG_PINNED != 0 {
            return false;
        }
        if matches!(
            crate::arena::classify_heap_generation(target),
            crate::arena::HeapGeneration::Old
        ) {
            // Pin-only retention: not moved, not traced, not marked.
            return pin_conservative_root_header(header);
        }
        (*header).gc_flags |= GC_FLAG_MARKED;
        push_mark_seed(header);
    }
    true
}

pub(super) fn try_mark_value_or_raw(word: u64, valid_ptrs: &ValidPointerSet) -> bool {
    // First try NaN-boxed interpretation (POINTER_TAG / STRING_TAG / BIGINT_TAG)
    if try_mark_value(word, valid_ptrs) {
        return true;
    }
    // Fallback: treat as raw (non-NaN-boxed) heap pointer.
    // Perry's is_string/is_array/is_pointer/is_closure locals store raw I64 addresses.
    // Validate against the known-heap-pointer set to avoid false positives from return addresses
    // and plain integers. Valid heap pointers are in the lower 48-bit address space and
    // won't have NaN-boxing tags in upper bits (already rejected above).
    let raw_ptr_u64 = word;
    if !(0x1000..=0x0000_FFFF_FFFF_FFFF).contains(&raw_ptr_u64) {
        return false; // Too small (null/invalid) or has upper bits set (NaN tag or non-address)
    }
    let raw_ptr = raw_ptr_u64 as usize;
    // Heap-range short-circuit: every valid raw heap pointer (object
    // start OR interior) must lie within [range_min, range_max + max
    // object size]. The interior-pointer case can land up to one
    // object-size past `range_max`, so we widen the upper bound by
    // an absolute slack to keep `enclosing_object` reachable for the
    // few real interior pointers that exist (`js_array_reduce`'s
    // `elements_ptr = arr + 8` shape, etc.). The slack is bounded by
    // the largest GcHeader.size field actually used — Perry's biggest
    // legitimate single allocation is a class instance with many
    // string fields, well under 4 KB. Anything larger came from a
    // pinned arena object (rare; doesn't reach this path) so 1 MB
    // gives plenty of headroom while still rejecting the typical
    // mis-tagged stack word.
    if !valid_ptrs.maybe_contains(raw_ptr)
        && raw_ptr.saturating_sub(0x10_0000) > valid_ptrs.range_max
    {
        return false;
    }
    // Try direct match first (pointer to object start).
    let target = if valid_ptrs.contains(&raw_ptr) {
        raw_ptr
    } else {
        // Issue #73: interior-pointer fallback. Runtime functions like
        // `js_array_reduce` derive `elements_ptr = arr + 8` and hold
        // only the interior pointer across user-callback invocations.
        // A conservative scan that only matches object-start addresses
        // would miss this, letting the GC sweep the backing array
        // mid-iteration. Look up the enclosing object and mark that.
        match valid_ptrs.enclosing_object(raw_ptr) {
            Some(start) => start,
            None => return false,
        }
    };
    unsafe {
        let header = header_from_user_ptr(target as *const u8);
        if (*header).gc_flags & GC_FLAG_MARKED != 0 {
            return false; // Already marked
        }
        if (*header).gc_flags & GC_FLAG_PINNED != 0 {
            return false; // Pinned objects are always live
        }
        (*header).gc_flags |= GC_FLAG_MARKED;
        push_mark_seed(header);
    }
    true
}

/// Specialized mark-and-enqueue for trace-phase field walks.
///
/// Descriptor-driven trace walks all share the same pattern: read a
/// heap-field word that is either a NaN-boxed JSValue or a raw I64
/// pointer at an object start, mark it if live, and push the marked
/// header onto the local worklist. The generic
/// `try_mark_value_or_raw` is general enough to also handle
/// conservative stack scans (raw interior pointers via
/// `enclosing_object`) and root scans (push to MARK_SEEDS so the
/// trace-marked-objects entry point can pick them up), but BOTH of
/// those features are pure overhead inside `drain_trace_worklist`:
///
/// 1. Field words never hold interior pointers — they're written via
///    `arr[i] = x` / `obj.f = x` / closure capture stores, all of
///    which use the object-start user pointer. Skipping
///    `enclosing_object` saves a binary-search lookup per field.
///
/// 2. The MARK_SEEDS push happens once per newly-marked object during
///    trace, but the same header is also pushed onto the local
///    worklist by the caller (so the trace drain visits it). The
///    extra MARK_SEEDS push goes onto a TLS vec, gets cleared at the
///    start of the next cycle, and is pure waste while we're already
///    in the trace phase. Skipping it saves a TLS slot deref +
///    Vec::push per marked object.
///
/// 3. The caller-side re-decode of the NaN-tag (to figure out
///    POINTER_MASK extraction vs raw-pointer extraction) is folded
///    into this function, so the caller doesn't pay that switch a
///    second time.
///
/// The valid-pointer hashset check is still load-bearing here — we
/// only elide the secondary `enclosing_object` fallback.
#[inline(always)]
#[cfg(target_os = "macos")]
pub(super) fn get_stack_bottom() -> usize {
    extern "C" {
        fn pthread_self() -> *mut std::ffi::c_void;
        fn pthread_get_stackaddr_np(thread: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    }
    unsafe {
        let thread = pthread_self();
        pthread_get_stackaddr_np(thread) as usize
    }
}

#[cfg(target_os = "linux")]
pub(super) fn get_stack_bottom() -> usize {
    extern "C" {
        fn pthread_self() -> usize;
        fn pthread_attr_init(attr: *mut [u64; 8]) -> i32;
        fn pthread_getattr_np(thread: usize, attr: *mut [u64; 8]) -> i32;
        fn pthread_attr_getstack(
            attr: *const [u64; 8],
            stackaddr: *mut *mut u8,
            stacksize: *mut usize,
        ) -> i32;
        fn pthread_attr_destroy(attr: *mut [u64; 8]) -> i32;
    }
    unsafe {
        let thread = pthread_self();
        let mut attr = [0u64; 8];
        pthread_attr_init(&mut attr);
        if pthread_getattr_np(thread, &mut attr) != 0 {
            return 0;
        }
        let mut stackaddr: *mut u8 = std::ptr::null_mut();
        let mut stacksize: usize = 0;
        pthread_attr_getstack(&attr, &mut stackaddr, &mut stacksize);
        pthread_attr_destroy(&mut attr);
        stackaddr as usize + stacksize
    }
}

// Windows: read TEB.StackBase. Works on every supported Windows version
// (Windows 7+) without needing GetCurrentThreadStackLimits (Win8+), so it
// stays correct on the `--min-windows-version=7` build path. The TEB lives
// at GS:[0] on x86_64 (FS:[0] on x86); StackBase sits at offset 0x08
// (the highest address — i.e. where the stack starts and grows down from).
// This is the same pointer kernel32!GetCurrentThreadStackLimits returns as
// `HighLimit`, just read directly from the TEB to avoid the kernel32 dep.
//
// Without this, conservative stack scan early-returns with stack_bottom=0,
// the GC sees no stack roots, and any heap pointer that lives only in a
// stack slot during a callback gets swept (issues #385/#386/#387 — the
// `Array.prototype.map` / `JSON.parse(...).property` / supported_features
// segfaults all traced back to here).
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
pub(super) fn get_stack_bottom() -> usize {
    let stack_base: usize;
    unsafe {
        std::arch::asm!(
            "mov {out}, gs:[0x08]",
            out = out(reg) stack_base,
            options(nostack, preserves_flags, readonly),
        );
    }
    stack_base
}

#[cfg(all(target_os = "windows", target_arch = "x86"))]
pub(super) fn get_stack_bottom() -> usize {
    let stack_base: usize;
    unsafe {
        std::arch::asm!(
            "mov {out}, fs:[0x04]",
            out = out(reg) stack_base,
            options(nostack, preserves_flags, readonly),
        );
    }
    stack_base
}

#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
pub(super) fn get_stack_bottom() -> usize {
    // ARM64 Windows: TEB pointer is in x18; StackBase at offset 0x08.
    let stack_base: usize;
    unsafe {
        let teb: usize;
        std::arch::asm!("mov {}, x18", out(reg) teb, options(nostack, preserves_flags, readonly));
        stack_base = *((teb + 0x08) as *const usize);
    }
    stack_base
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "linux",
    all(
        target_os = "windows",
        any(target_arch = "x86_64", target_arch = "x86", target_arch = "aarch64")
    ),
)))]
pub(super) fn get_stack_bottom() -> usize {
    0 // Stack scanning not supported on this OS/arch
}

pub(super) enum RuntimeRootVisitMode<'a> {
    Mark {
        valid_ptrs: &'a ValidPointerSet,
    },
    CopyingCheck {
        checker: &'a mut CopyingNurseryPreflight,
    },
    CopyingMark {
        collector: &'a mut CopyingNurseryCollector,
    },
    CopyingRewrite {
        collector: &'a CopyingNurseryCollector,
    },
    Rewrite {
        valid_ptrs: &'a ValidPointerSet,
    },
    Verify {
        valid_ptrs: &'a ValidPointerSet,
        surface: &'static str,
    },
    Copy {
        mark: &'a mut dyn FnMut(f64),
    },
}

/// Mutable runtime-root visitor used by GC-owned scanner families.
///
/// A scanner calls the slot method that matches its storage. During mark,
/// root slots mark their current referent. During evacuation rewrite, the
/// same scanner is revisited and any forwarded referent is written back to
/// the runtime-owned slot. Compatibility copy mode powers the legacy
/// `scan_*_roots(mark)` wrappers.
pub struct RuntimeRootVisitor<'a> {
    pub(super) mode: RuntimeRootVisitMode<'a>,
    pub(super) root_source_stats: Option<*mut RootSourceSlotTraceStats>,
}

impl<'a> RuntimeRootVisitor<'a> {
    pub(super) fn for_mark(valid_ptrs: &'a ValidPointerSet) -> Self {
        Self {
            mode: RuntimeRootVisitMode::Mark { valid_ptrs },
            root_source_stats: None,
        }
    }

    pub(crate) fn for_rewrite(valid_ptrs: &'a ValidPointerSet) -> Self {
        Self {
            mode: RuntimeRootVisitMode::Rewrite { valid_ptrs },
            root_source_stats: None,
        }
    }

    pub(super) fn for_copying_check(checker: &'a mut CopyingNurseryPreflight) -> Self {
        Self {
            mode: RuntimeRootVisitMode::CopyingCheck { checker },
            root_source_stats: None,
        }
    }

    pub(super) fn for_copying_mark(collector: &'a mut CopyingNurseryCollector) -> Self {
        Self {
            mode: RuntimeRootVisitMode::CopyingMark { collector },
            root_source_stats: None,
        }
    }

    pub(super) fn for_copying_rewrite(collector: &'a CopyingNurseryCollector) -> Self {
        Self {
            mode: RuntimeRootVisitMode::CopyingRewrite { collector },
            root_source_stats: None,
        }
    }

    pub(super) fn for_verify(valid_ptrs: &'a ValidPointerSet, surface: &'static str) -> Self {
        Self {
            mode: RuntimeRootVisitMode::Verify {
                valid_ptrs,
                surface,
            },
            root_source_stats: None,
        }
    }

    pub fn for_copy(mark: &'a mut dyn FnMut(f64)) -> Self {
        Self {
            mode: RuntimeRootVisitMode::Copy { mark },
            root_source_stats: None,
        }
    }

    #[inline]
    pub(super) fn set_root_source_stats(
        &mut self,
        stats: Option<*mut RootSourceSlotTraceStats>,
    ) -> Option<*mut RootSourceSlotTraceStats> {
        std::mem::replace(&mut self.root_source_stats, stats)
    }

    #[inline]
    pub(super) fn record_source_scan_bits(&mut self, bits: u64) {
        if let Some(stats) = self.root_source_stats {
            unsafe {
                (*stats).record_scan(bits != 0, root_slot_pointer_candidate(bits));
            }
        }
    }

    #[inline]
    pub(super) fn record_source_scan_addr(&mut self, addr: usize) {
        if let Some(stats) = self.root_source_stats {
            unsafe {
                (*stats).record_scan(addr != 0, addr != 0);
            }
        }
    }

    #[inline]
    pub(super) fn record_source_rewrite(&mut self) {
        if let Some(stats) = self.root_source_stats {
            unsafe {
                (*stats).record_rewrite();
            }
        }
    }

    /// True during post-move fixup/verification passes where
    /// metadata-only pointer keys can be rewritten without making those
    /// keys roots.
    pub fn is_metadata_rewrite_phase(&self) -> bool {
        matches!(
            &self.mode,
            RuntimeRootVisitMode::Rewrite { .. }
                | RuntimeRootVisitMode::CopyingRewrite { .. }
                | RuntimeRootVisitMode::Verify { .. }
        )
    }

    #[inline]
    pub(super) fn visit_nanbox_bits(&mut self, bits: u64) -> Option<u64> {
        match &mut self.mode {
            RuntimeRootVisitMode::Mark { valid_ptrs } => {
                try_mark_value(bits, valid_ptrs);
                None
            }
            RuntimeRootVisitMode::CopyingCheck { checker } => {
                checker.check_bits(bits);
                None
            }
            RuntimeRootVisitMode::CopyingMark { collector } => collector.visit_value_bits(bits),
            RuntimeRootVisitMode::CopyingRewrite { collector } => {
                collector.rewrite_value_bits(bits)
            }
            RuntimeRootVisitMode::Rewrite { valid_ptrs } => {
                try_rewrite_nanboxed_value(bits, valid_ptrs)
            }
            RuntimeRootVisitMode::Verify {
                valid_ptrs,
                surface,
            } => {
                if let Some(new_bits) = try_rewrite_nanboxed_value(bits, valid_ptrs) {
                    panic_stale_forwarded_reference(surface, 0, bits, new_bits);
                }
                None
            }
            RuntimeRootVisitMode::Copy { mark } => {
                (*mark)(f64::from_bits(bits));
                None
            }
        }
    }

    #[inline]
    pub(super) fn visit_heap_word_bits(&mut self, bits: u64) -> Option<u64> {
        match &mut self.mode {
            RuntimeRootVisitMode::Mark { valid_ptrs } => {
                try_mark_value_or_raw(bits, valid_ptrs);
                None
            }
            RuntimeRootVisitMode::CopyingCheck { checker } => {
                checker.check_bits(bits);
                None
            }
            RuntimeRootVisitMode::CopyingMark { collector } => collector.visit_value_bits(bits),
            RuntimeRootVisitMode::CopyingRewrite { collector } => {
                collector.rewrite_value_bits(bits)
            }
            RuntimeRootVisitMode::Rewrite { valid_ptrs } => try_rewrite_value(bits, valid_ptrs),
            RuntimeRootVisitMode::Verify {
                valid_ptrs,
                surface,
            } => {
                if let Some(new_bits) = try_rewrite_value(bits, valid_ptrs) {
                    panic_stale_forwarded_reference(surface, 0, bits, new_bits);
                }
                None
            }
            RuntimeRootVisitMode::Copy { mark } => {
                let tag = bits & TAG_MASK;
                if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
                    (*mark)(f64::from_bits(bits));
                } else if tag < 0x7FF8_0000_0000_0000
                    && (0x1000..=0x0000_FFFF_FFFF_FFFF).contains(&bits)
                {
                    (*mark)(f64::from_bits(POINTER_TAG | (bits & POINTER_MASK)));
                }
                None
            }
        }
    }

    #[inline]
    pub(super) fn visit_tagged_raw_addr(&mut self, addr: usize, copy_tag: u64) -> Option<usize> {
        if addr == 0 {
            return None;
        }
        match &mut self.mode {
            RuntimeRootVisitMode::Mark { valid_ptrs } => {
                try_mark_raw_root_addr(addr, valid_ptrs);
                None
            }
            RuntimeRootVisitMode::CopyingCheck { checker } => {
                checker.check_addr(addr);
                None
            }
            RuntimeRootVisitMode::CopyingMark { collector } => collector.visit_raw_addr(addr),
            RuntimeRootVisitMode::CopyingRewrite { collector } => collector.rewrite_raw_addr(addr),
            RuntimeRootVisitMode::Rewrite { valid_ptrs } => try_rewrite_raw_addr(addr, valid_ptrs),
            RuntimeRootVisitMode::Verify {
                valid_ptrs,
                surface,
            } => {
                if let Some(new_addr) = try_rewrite_raw_addr(addr, valid_ptrs) {
                    panic_stale_forwarded_reference(
                        surface,
                        0,
                        copy_tag | (addr as u64 & POINTER_MASK),
                        copy_tag | (new_addr as u64 & POINTER_MASK),
                    );
                }
                None
            }
            RuntimeRootVisitMode::Copy { mark } => {
                (*mark)(f64::from_bits(copy_tag | (addr as u64 & POINTER_MASK)));
                None
            }
        }
    }

    #[inline]
    pub(super) fn visit_metadata_raw_addr(&mut self, addr: usize) -> Option<usize> {
        if addr == 0 {
            return None;
        }
        match &mut self.mode {
            RuntimeRootVisitMode::Rewrite { valid_ptrs } => try_rewrite_raw_addr(addr, valid_ptrs),
            RuntimeRootVisitMode::CopyingCheck { .. } => None,
            RuntimeRootVisitMode::CopyingMark { .. } => None,
            RuntimeRootVisitMode::CopyingRewrite { collector } => collector.rewrite_raw_addr(addr),
            RuntimeRootVisitMode::Verify {
                valid_ptrs,
                surface,
            } => {
                if let Some(new_addr) = try_rewrite_raw_addr(addr, valid_ptrs) {
                    panic_stale_forwarded_reference(surface, 0, addr as u64, new_addr as u64);
                }
                None
            }
            RuntimeRootVisitMode::Mark { .. } | RuntimeRootVisitMode::Copy { .. } => None,
        }
    }

    /// Visit a mutable NaN-boxed JSValue stored as `f64`.
    /// Returns true when rewrite mode changed the slot.
    pub fn visit_nanbox_f64_slot(&mut self, slot: &mut f64) -> bool {
        let bits = slot.to_bits();
        self.record_source_scan_bits(bits);
        if let Some(new_bits) = self.visit_nanbox_bits(bits) {
            *slot = f64::from_bits(new_bits);
            self.record_source_rewrite();
            true
        } else {
            false
        }
    }

    /// Visit a mutable NaN-boxed JSValue stored as `u64` bits.
    /// Returns true when rewrite mode changed the slot.
    pub fn visit_nanbox_u64_slot(&mut self, slot: &mut u64) -> bool {
        self.record_source_scan_bits(*slot);
        if let Some(new_bits) = self.visit_nanbox_bits(*slot) {
            *slot = new_bits;
            self.record_source_rewrite();
            true
        } else {
            false
        }
    }

    /// Visit an atomic root slot containing NaN-boxed JSValue bits.
    pub fn visit_atomic_nanbox_u64_slot(
        &mut self,
        slot: &std::sync::atomic::AtomicU64,
        load_ordering: std::sync::atomic::Ordering,
        store_ordering: std::sync::atomic::Ordering,
    ) -> bool {
        let current = slot.load(load_ordering);
        self.record_source_scan_bits(current);
        if let Some(new_bits) = self.visit_nanbox_bits(current) {
            slot.store(new_bits, atomic_store_ordering(store_ordering));
            self.record_source_rewrite();
            true
        } else {
            false
        }
    }

    /// Visit a mutable heap word that may store either a NaN-boxed JSValue
    /// pointer or a raw heap pointer.
    ///
    /// This matches heap-field rewrite semantics for runtime-owned caches
    /// whose keys are bit copies of closure captures or object fields.
    pub fn visit_heap_word_u64_slot(&mut self, slot: &mut u64) -> bool {
        self.record_source_scan_bits(*slot);
        if let Some(new_bits) = self.visit_heap_word_bits(*slot) {
            *slot = new_bits;
            self.record_source_rewrite();
            true
        } else {
            false
        }
    }

    /// Visit a raw `f64` slot address when the owner cannot hand out a
    /// Rust `&mut f64` (for example `static mut` storage).
    ///
    /// # Safety
    /// `slot` must be valid for a read and, in rewrite mode, a write.
    pub unsafe fn visit_nanbox_f64_raw_slot(&mut self, slot: *mut f64) -> bool {
        if slot.is_null() {
            return false;
        }
        let bits = (*slot).to_bits();
        self.record_source_scan_bits(bits);
        if let Some(new_bits) = self.visit_nanbox_bits(bits) {
            *slot = f64::from_bits(new_bits);
            self.record_source_rewrite();
            true
        } else {
            false
        }
    }

    /// Visit a raw `u64` slot address when the owner cannot hand out a
    /// Rust `&mut u64`.
    ///
    /// # Safety
    /// `slot` must be valid for a read and, in rewrite mode, a write.
    pub unsafe fn visit_nanbox_u64_raw_slot(&mut self, slot: *mut u64) -> bool {
        if slot.is_null() {
            return false;
        }
        self.record_source_scan_bits(*slot);
        if let Some(new_bits) = self.visit_nanbox_bits(*slot) {
            *slot = new_bits;
            self.record_source_rewrite();
            true
        } else {
            false
        }
    }

    /// Visit a `Cell<f64>` that stores a NaN-boxed JSValue.
    pub fn visit_cell_f64_slot(&mut self, slot: &Cell<f64>) -> bool {
        let bits = slot.get().to_bits();
        self.record_source_scan_bits(bits);
        if let Some(new_bits) = self.visit_nanbox_bits(bits) {
            slot.set(f64::from_bits(new_bits));
            self.record_source_rewrite();
            true
        } else {
            false
        }
    }

    /// Visit a root slot that stores a raw mutable heap pointer.
    pub fn visit_raw_mut_ptr_slot<T>(&mut self, slot: &mut *mut T) -> bool {
        self.record_source_scan_addr(*slot as usize);
        if let Some(new_addr) = self.visit_tagged_raw_addr(*slot as usize, POINTER_TAG) {
            *slot = new_addr as *mut T;
            self.record_source_rewrite();
            true
        } else {
            false
        }
    }

    /// Visit a root slot that stores a raw const heap pointer.
    pub fn visit_raw_const_ptr_slot<T>(&mut self, slot: &mut *const T) -> bool {
        self.record_source_scan_addr(*slot as usize);
        if let Some(new_addr) = self.visit_tagged_raw_addr(*slot as usize, POINTER_TAG) {
            *slot = new_addr as *const T;
            self.record_source_rewrite();
            true
        } else {
            false
        }
    }

    /// Visit a raw const heap pointer slot, using a specific NaN-box tag
    /// when the visitor is running in compatibility copy mode.
    pub fn visit_tagged_raw_const_ptr_slot<T>(&mut self, slot: &mut *const T, tag: u64) -> bool {
        self.record_source_scan_addr(*slot as usize);
        if let Some(new_addr) = self.visit_tagged_raw_addr(*slot as usize, tag) {
            *slot = new_addr as *const T;
            self.record_source_rewrite();
            true
        } else {
            false
        }
    }

    /// Visit a root slot that stores a raw heap pointer as `usize`.
    pub fn visit_usize_slot(&mut self, slot: &mut usize) -> bool {
        self.record_source_scan_addr(*slot);
        if let Some(new_addr) = self.visit_tagged_raw_addr(*slot, POINTER_TAG) {
            *slot = new_addr;
            self.record_source_rewrite();
            true
        } else {
            false
        }
    }

    /// Visit a raw heap pointer stored as `usize`, using a specific
    /// NaN-box tag when the visitor is running in compatibility copy mode.
    pub fn visit_tagged_usize_slot(&mut self, slot: &mut usize, tag: u64) -> bool {
        self.record_source_scan_addr(*slot);
        if let Some(new_addr) = self.visit_tagged_raw_addr(*slot, tag) {
            *slot = new_addr;
            self.record_source_rewrite();
            true
        } else {
            false
        }
    }

    /// Visit a root slot that stores a raw heap pointer as `i64`.
    pub fn visit_i64_slot(&mut self, slot: &mut i64) -> bool {
        self.record_source_scan_addr(if *slot > 0 { *slot as usize } else { 0 });
        if *slot <= 0 {
            return false;
        }
        if let Some(new_addr) = self.visit_tagged_raw_addr(*slot as usize, POINTER_TAG) {
            *slot = new_addr as i64;
            self.record_source_rewrite();
            true
        } else {
            false
        }
    }

    /// Visit a raw `usize` slot address.
    ///
    /// # Safety
    /// `slot` must be valid for a read and, in rewrite mode, a write.
    pub unsafe fn visit_usize_raw_slot(&mut self, slot: *mut usize) -> bool {
        if slot.is_null() {
            return false;
        }
        self.record_source_scan_addr(*slot);
        if let Some(new_addr) = self.visit_tagged_raw_addr(*slot, POINTER_TAG) {
            *slot = new_addr;
            self.record_source_rewrite();
            true
        } else {
            false
        }
    }

    /// Visit an atomic raw pointer root slot.
    pub fn visit_atomic_raw_mut_ptr_slot<T>(
        &mut self,
        slot: &std::sync::atomic::AtomicPtr<T>,
        load_ordering: std::sync::atomic::Ordering,
        store_ordering: std::sync::atomic::Ordering,
    ) -> bool {
        let current = slot.load(load_ordering);
        self.record_source_scan_addr(current as usize);
        if let Some(new_addr) = self.visit_tagged_raw_addr(current as usize, POINTER_TAG) {
            slot.store(new_addr as *mut T, atomic_store_ordering(store_ordering));
            self.record_source_rewrite();
            true
        } else {
            false
        }
    }

    /// Visit an atomic `i64` root slot containing a raw heap pointer.
    pub fn visit_atomic_i64_slot(
        &mut self,
        slot: &std::sync::atomic::AtomicI64,
        load_ordering: std::sync::atomic::Ordering,
        store_ordering: std::sync::atomic::Ordering,
    ) -> bool {
        let current = slot.load(load_ordering);
        self.record_source_scan_addr(if current > 0 { current as usize } else { 0 });
        if current <= 0 {
            return false;
        }
        if let Some(new_addr) = self.visit_tagged_raw_addr(current as usize, POINTER_TAG) {
            slot.store(new_addr as i64, atomic_store_ordering(store_ordering));
            self.record_source_rewrite();
            true
        } else {
            false
        }
    }

    /// Visit a metadata-only raw heap pointer key. The value is rewritten
    /// if forwarded, but it is not marked as a root. Mark/copy modes emit
    /// nothing; post-copy rewrite only follows forwarding pointers that
    /// already exist.
    pub fn visit_metadata_usize_slot(&mut self, slot: &mut usize) -> bool {
        if let Some(new_addr) = self.visit_metadata_raw_addr(*slot) {
            *slot = new_addr;
            true
        } else {
            false
        }
    }

    /// Visit a metadata-only raw heap pointer key stored as `i64`.
    pub fn visit_metadata_i64_slot(&mut self, slot: &mut i64) -> bool {
        if *slot <= 0 {
            return false;
        }
        if let Some(new_addr) = self.visit_metadata_raw_addr(*slot as usize) {
            *slot = new_addr as i64;
            true
        } else {
            false
        }
    }

    /// Visit a raw metadata-only `usize` slot address.
    ///
    /// # Safety
    /// `slot` must be valid for a read and, in rewrite mode, a write.
    pub unsafe fn visit_metadata_usize_raw_slot(&mut self, slot: *mut usize) -> bool {
        if slot.is_null() {
            return false;
        }
        if let Some(new_addr) = self.visit_metadata_raw_addr(*slot) {
            *slot = new_addr;
            true
        } else {
            false
        }
    }
}

#[inline]
pub(super) fn atomic_store_ordering(
    ordering: std::sync::atomic::Ordering,
) -> std::sync::atomic::Ordering {
    match ordering {
        std::sync::atomic::Ordering::Relaxed => std::sync::atomic::Ordering::Relaxed,
        std::sync::atomic::Ordering::Acquire | std::sync::atomic::Ordering::Release => {
            std::sync::atomic::Ordering::Release
        }
        std::sync::atomic::Ordering::AcqRel => std::sync::atomic::Ordering::Release,
        std::sync::atomic::Ordering::SeqCst => std::sync::atomic::Ordering::SeqCst,
        _ => std::sync::atomic::Ordering::SeqCst,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MutableRootSlotKind {
    ShadowStack,
    GlobalRoot,
}

#[derive(Clone, Copy)]
pub(super) struct MutableRootSlot {
    pub(super) kind: MutableRootSlotKind,
    pub(super) ptr: *mut u64,
}

impl MutableRootSlot {
    #[inline]
    pub(super) unsafe fn read(self) -> u64 {
        *self.ptr
    }

    #[inline]
    pub(super) unsafe fn write(self, bits: u64) {
        *self.ptr = bits;
    }
}

/// Visit every live shadow-stack slot. The visitor receives real
/// mutable slot addresses so the same walk can support mark-only
/// scanning and post-forwarding rewrites.
pub(super) fn visit_shadow_stack_root_slots(mut visit: impl FnMut(MutableRootSlot)) {
    SHADOW.with(|cell| unsafe {
        let s = &mut *cell.get();
        if s.stack.is_empty() {
            return;
        }
        let mut top = s.frame_top;
        while top != usize::MAX && top >= SHADOW_STACK_HEADER_SLOTS {
            let header_base = top - SHADOW_STACK_HEADER_SLOTS;
            if header_base + 1 >= s.stack.len() {
                break;
            }
            let slot_count = s.stack[header_base + 1] as usize;
            let slots_end = top + slot_count;
            if slots_end > s.stack.len() {
                break;
            }
            let base = s.stack.as_mut_ptr().add(top);
            for i in 0..slot_count {
                let slot_idx = top + i;
                if !s.active.get(slot_idx).copied().unwrap_or(false) {
                    continue;
                }
                let bound_ptr = s.slot_ptrs.get(slot_idx).copied().unwrap_or(0) as *mut u64;
                let ptr = if bound_ptr.is_null() {
                    base.add(i)
                } else {
                    bound_ptr
                };
                visit(MutableRootSlot {
                    kind: MutableRootSlotKind::ShadowStack,
                    ptr,
                });
            }
            top = s.stack[header_base] as usize;
        }
    });
}

/// Visit every registered module-global root slot.
pub(super) fn visit_global_root_slots(mut visit: impl FnMut(MutableRootSlot)) {
    GLOBAL_ROOTS.with(|roots| {
        let roots = roots.borrow();
        for &root_ptr in roots.iter() {
            if root_ptr.is_null() {
                continue;
            }
            visit(MutableRootSlot {
                kind: MutableRootSlotKind::GlobalRoot,
                ptr: root_ptr,
            });
        }
    });
}

/// Visit the root slots whose storage is owned by this runtime and can
/// therefore be rewritten after evacuation.
pub(super) fn visit_mutable_root_slots(mut visit: impl FnMut(MutableRootSlot)) {
    visit_shadow_stack_root_slots(&mut visit);
    visit_global_root_slots(&mut visit);
}

#[derive(Default)]
pub(super) struct MutableRootSlotScanCursor {
    shadow_seen: usize,
    global_seen: usize,
    shadow_done: bool,
    global_done: bool,
}

pub(super) fn mark_mutable_root_slots_step(
    valid_ptrs: &ValidPointerSet,
    mut shadow_stats: Option<&mut ShadowRootTraceStats>,
    mut root_sources: Option<&mut RootSourcesTraceStats>,
    cursor: &mut MutableRootSlotScanCursor,
    budget: usize,
) -> bool {
    let mut remaining = budget;
    if !cursor.shadow_done {
        if remaining == 0 {
            return false;
        }
        let mut seen = 0usize;
        let mut exhausted = true;
        visit_shadow_stack_root_slots(|slot| unsafe {
            if seen < cursor.shadow_seen {
                seen += 1;
                return;
            }
            if remaining == 0 {
                exhausted = false;
                return;
            }

            let bits = slot.read();
            record_mutable_slot_scan_source(slot, bits, valid_ptrs, &mut root_sources);
            if let Some(stats) = shadow_stats.as_mut() {
                stats.record_scan(bits);
            }
            if bits != 0 {
                try_mark_value(bits, valid_ptrs);
            }
            seen += 1;
            cursor.shadow_seen = seen;
            remaining -= 1;
        });
        if !exhausted {
            return false;
        }
        cursor.shadow_done = true;
    }

    if !cursor.global_done {
        if remaining == 0 {
            return false;
        }
        let mut seen = 0usize;
        let mut exhausted = true;
        visit_global_root_slots(|slot| unsafe {
            if seen < cursor.global_seen {
                seen += 1;
                return;
            }
            if remaining == 0 {
                exhausted = false;
                return;
            }

            let bits = slot.read();
            record_mutable_slot_scan_source(slot, bits, valid_ptrs, &mut root_sources);
            if bits != 0 {
                mark_global_root_bits(bits, valid_ptrs);
            }
            seen += 1;
            cursor.global_seen = seen;
            remaining -= 1;
        });
        if !exhausted {
            return false;
        }
        cursor.global_done = true;
    }

    true
}

#[inline]
pub(super) fn shadow_slot_pointer_root(bits: u64) -> bool {
    let tag = bits & TAG_MASK;
    let addr = bits & POINTER_MASK;
    addr != 0 && (tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG)
}

#[inline]
pub(super) fn root_slot_pointer_candidate(bits: u64) -> bool {
    if shadow_slot_pointer_root(bits) {
        return true;
    }
    let tag = bits & TAG_MASK;
    tag < 0x7FF8_0000_0000_0000 && CopyingPointerSet::raw_pointer_candidate(bits)
}

#[inline]
pub(super) fn mutable_slot_points_to_valid_root(bits: u64, valid_ptrs: &ValidPointerSet) -> bool {
    if shadow_slot_pointer_root(bits) {
        let addr = (bits & POINTER_MASK) as usize;
        return valid_ptrs.contains(&addr);
    }
    let raw_ptr = bits as usize;
    raw_ptr != 0 && valid_ptrs.contains(&raw_ptr)
}

#[inline]
pub(super) fn root_source_for_mutable_slot(
    sources: &mut RootSourcesTraceStats,
    kind: MutableRootSlotKind,
) -> &mut RootSourceSlotTraceStats {
    match kind {
        MutableRootSlotKind::ShadowStack => &mut sources.compiled_shadow,
        MutableRootSlotKind::GlobalRoot => &mut sources.module_globals,
    }
}

#[inline]
pub(super) fn record_mutable_slot_scan_source(
    slot: MutableRootSlot,
    bits: u64,
    valid_ptrs: &ValidPointerSet,
    root_sources: &mut Option<&mut RootSourcesTraceStats>,
) {
    if let Some(sources) = root_sources {
        root_source_for_mutable_slot(sources, slot.kind).record_scan(
            bits != 0,
            mutable_slot_points_to_valid_root(bits, valid_ptrs),
        );
    }
}

#[inline]
pub(super) fn record_mutable_slot_rewrite_source(
    slot: MutableRootSlot,
    root_sources: &mut Option<&mut RootSourcesTraceStats>,
) {
    if let Some(sources) = root_sources {
        root_source_for_mutable_slot(sources, slot.kind).record_rewrite();
    }
}

#[inline]
pub(super) fn mark_global_root_bits(bits: u64, valid_ptrs: &ValidPointerSet) {
    // First try NaN-boxed interpretation (exported globals, closures, etc.).
    if try_mark_value(bits, valid_ptrs) {
        return;
    }
    // Module variable globals store raw I64 pointers (not NaN-boxed).
    // Preserve the historical direct-object-start behavior: validate
    // against valid_ptrs and mark the target, without the conservative
    // interior-pointer fallback used by stack scanning.
    let raw_ptr = bits as usize;
    try_mark_raw_root_addr(raw_ptr, valid_ptrs);
}

/// Mark mutable roots (shadow-stack slots and registered globals).
#[allow(dead_code)]
pub(super) fn mark_mutable_root_slots(
    valid_ptrs: &ValidPointerSet,
    mut shadow_stats: Option<&mut ShadowRootTraceStats>,
    mut root_sources: Option<&mut RootSourcesTraceStats>,
) {
    visit_mutable_root_slots(|slot| unsafe {
        let bits = slot.read();
        record_mutable_slot_scan_source(slot, bits, valid_ptrs, &mut root_sources);
        if matches!(slot.kind, MutableRootSlotKind::ShadowStack) {
            if let Some(stats) = shadow_stats.as_mut() {
                stats.record_scan(bits);
            }
        }
        if bits == 0 {
            return;
        }
        match slot.kind {
            MutableRootSlotKind::ShadowStack => {
                try_mark_value(bits, valid_ptrs);
            }
            MutableRootSlotKind::GlobalRoot => mark_global_root_bits(bits, valid_ptrs),
        }
    });
}

#[inline]
pub(super) fn nanboxed_root_header(
    value_bits: u64,
    valid_ptrs: &ValidPointerSet,
) -> Option<*mut GcHeader> {
    let tag = value_bits & TAG_MASK;
    if tag != POINTER_TAG && tag != STRING_TAG && tag != BIGINT_TAG {
        return None;
    }
    let ptr_val = (value_bits & POINTER_MASK) as usize;
    if ptr_val == 0 || !valid_ptrs.maybe_contains(ptr_val) || !valid_ptrs.contains(&ptr_val) {
        return None;
    }
    Some(unsafe { header_from_user_ptr(ptr_val as *const u8) })
}

#[inline]
pub(super) fn pin_conservative_root_header(header: *mut GcHeader) -> bool {
    CONS_PINNED.with(|s| {
        let mut pinned = s.borrow_mut();
        pinned.insert(header as usize)
    })
}

#[inline]
pub(super) fn mark_copy_only_scanner_bits(
    bits: u64,
    valid_ptrs: &ValidPointerSet,
    pin_discoveries: bool,
) -> Option<usize> {
    let Some(header) = nanboxed_root_header(bits, valid_ptrs) else {
        return None;
    };
    unsafe {
        let flags = (*header).gc_flags;
        if flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0 {
            (*header).gc_flags = flags | GC_FLAG_MARKED;
            push_mark_seed(header);
        }
    }
    if pin_discoveries && pin_conservative_root_header(header) {
        return Some(unsafe { (*header).size as usize });
    }
    None
}

#[inline]
pub(super) fn record_copy_only_scanner_mark_emission(
    bits: u64,
    valid_ptrs: &ValidPointerSet,
    legacy_stats: &mut LegacyRootTraceStats,
) {
    legacy_stats.emitted_roots += 1;
    let Some(header) = nanboxed_root_header(bits, valid_ptrs) else {
        legacy_stats.malformed_roots += 1;
        return;
    };
    let user = unsafe { (header as *mut u8).add(GC_HEADER_SIZE) as usize };
    if crate::arena::pointer_in_nursery(user) {
        legacy_stats.emitted_young_roots += 1;
    } else if MALLOC_STATE.with(|s| s.borrow().objects.contains(&header)) {
        legacy_stats.emitted_malloc_roots += 1;
    } else {
        legacy_stats.emitted_old_roots += 1;
    }
}

pub(super) struct RegisteredRootMarkContext {
    pub(super) valid_ptrs: *const ValidPointerSet,
    pub(super) pin_discoveries: bool,
    pub(super) legacy_stats: *mut LegacyRootTraceStats,
}

pub(super) extern "C" fn perry_ffi_visit_mutable_root_slot(
    kind: u32,
    slot: *mut c_void,
    ctx: *mut c_void,
) -> bool {
    if slot.is_null() || ctx.is_null() {
        return false;
    }
    let visitor = unsafe { &mut *(ctx as *mut RuntimeRootVisitor<'_>) };
    unsafe {
        match kind {
            PERRY_FFI_ROOT_SLOT_I64 => visitor.visit_i64_slot(&mut *(slot as *mut i64)),
            PERRY_FFI_ROOT_SLOT_USIZE => visitor.visit_usize_slot(&mut *(slot as *mut usize)),
            PERRY_FFI_ROOT_SLOT_RAW_MUT_PTR => {
                visitor.visit_raw_mut_ptr_slot(&mut *(slot as *mut *mut c_void))
            }
            PERRY_FFI_ROOT_SLOT_NANBOX_F64 => {
                visitor.visit_nanbox_f64_slot(&mut *(slot as *mut f64))
            }
            PERRY_FFI_ROOT_SLOT_NANBOX_U64 => {
                visitor.visit_nanbox_u64_slot(&mut *(slot as *mut u64))
            }
            _ => false,
        }
    }
}

pub(super) fn visit_ffi_mutable_registered_roots(visitor: &mut RuntimeRootVisitor<'_>) {
    visit_ffi_mutable_registered_roots_with_sources(visitor, None);
}

pub(super) fn visit_ffi_mutable_registered_roots_with_sources(
    visitor: &mut RuntimeRootVisitor<'_>,
    mut root_sources: Option<&mut RootSourcesTraceStats>,
) {
    let scanners: Vec<PerryFfiMutableRootScanner> =
        FFI_MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone());
    let named_scanners: Vec<(PerryFfiNamedMutableRootScanner, usize)> =
        FFI_NAMED_MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone());
    let stats = match &mut root_sources {
        Some(sources) => {
            sources
                .ffi_mutable_scanners
                .record_registered_scanners(scanners.len() + named_scanners.len());
            Some(&mut sources.ffi_mutable_scanners as *mut RootSourceSlotTraceStats)
        }
        None => None,
    };
    let ctx = visitor as *mut RuntimeRootVisitor<'_> as *mut c_void;
    let previous = visitor.set_root_source_stats(stats);
    for scanner in scanners {
        scanner(perry_ffi_visit_mutable_root_slot, ctx);
    }
    for (scanner, scanner_id) in named_scanners {
        scanner(scanner_id, perry_ffi_visit_mutable_root_slot, ctx);
    }
    visitor.set_root_source_stats(previous);
}

/// Run registered runtime-owned scanners that expose mutable slots.
#[allow(dead_code)]
pub(super) fn mark_mutable_registered_roots(valid_ptrs: &ValidPointerSet) {
    mark_mutable_registered_roots_with_sources(valid_ptrs, None);
}

#[allow(dead_code)]
pub(super) fn mark_mutable_registered_roots_with_sources(
    valid_ptrs: &ValidPointerSet,
    mut root_sources: Option<&mut RootSourcesTraceStats>,
) {
    let scanners: Vec<MutableRootScannerEntry> = MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone());
    if let Some(sources) = &mut root_sources {
        sources.runtime_handles.record_registered_scanners(
            scanners
                .iter()
                .filter(|entry| entry.source == MutableRootScannerSource::RuntimeHandles)
                .count(),
        );
        sources.runtime_mutable_scanners.record_registered_scanners(
            scanners
                .iter()
                .filter(|entry| entry.source == MutableRootScannerSource::RuntimeMutableScanner)
                .count(),
        );
    }
    let mut visitor = RuntimeRootVisitor::for_mark(valid_ptrs);
    for entry in scanners {
        let stats = match &mut root_sources {
            Some(sources) => match entry.source {
                MutableRootScannerSource::RuntimeHandles => {
                    Some(&mut sources.runtime_handles as *mut RootSourceSlotTraceStats)
                }
                MutableRootScannerSource::RuntimeMutableScanner => {
                    Some(&mut sources.runtime_mutable_scanners as *mut RootSourceSlotTraceStats)
                }
            },
            None => None,
        };
        let previous = visitor.set_root_source_stats(stats);
        (entry.scanner)(&mut visitor);
        visitor.set_root_source_stats(previous);
    }
    visit_ffi_mutable_registered_roots_with_sources(&mut visitor, root_sources);
}

/// Run legacy copy-only root scanners. When evacuation is enabled,
/// every discovered root is pinned because the scanner API gives us no
/// slot to rewrite after forwarding.
#[allow(dead_code)]
pub(super) fn mark_registered_roots(
    valid_ptrs: &ValidPointerSet,
    pin_discoveries: bool,
) -> LegacyRootTraceStats {
    let mut legacy_stats = LegacyRootTraceStats::default();
    // Collect scanners first to avoid borrow conflicts
    let scanners: Vec<fn(&mut dyn FnMut(f64))> = ROOT_SCANNERS.with(|s| s.borrow().clone());
    let ffi_scanners: Vec<PerryFfiRootScanner> = FFI_ROOT_SCANNERS.with(|s| s.borrow().clone());
    legacy_stats.registered_rust_scanners = scanners.len();
    legacy_stats.registered_ffi_scanners = ffi_scanners.len();

    for scanner in scanners {
        scanner(&mut |value: f64| {
            record_copy_only_scanner_mark_emission(value.to_bits(), valid_ptrs, &mut legacy_stats);
            if let Some(bytes) =
                mark_copy_only_scanner_bits(value.to_bits(), valid_ptrs, pin_discoveries)
            {
                legacy_stats.pinned_roots += 1;
                legacy_stats.pinned_bytes += bytes;
            }
        });
    }

    let mut ctx = RegisteredRootMarkContext {
        valid_ptrs: valid_ptrs as *const ValidPointerSet,
        pin_discoveries,
        legacy_stats: &mut legacy_stats as *mut LegacyRootTraceStats,
    };
    let ctx = &mut ctx as *mut RegisteredRootMarkContext as *mut c_void;
    for scanner in ffi_scanners {
        scanner(perry_ffi_mark_root, ctx);
    }
    legacy_stats
}

pub(super) extern "C" fn perry_ffi_mark_root(value: f64, ctx: *mut c_void) {
    if ctx.is_null() {
        return;
    }
    let ctx = unsafe { &*(ctx as *const RegisteredRootMarkContext) };
    if ctx.valid_ptrs.is_null() {
        return;
    }
    let valid_ptrs = unsafe { &*ctx.valid_ptrs };
    if !ctx.legacy_stats.is_null() {
        unsafe {
            record_copy_only_scanner_mark_emission(
                value.to_bits(),
                valid_ptrs,
                &mut *ctx.legacy_stats,
            );
        }
    }
    if let Some(bytes) =
        mark_copy_only_scanner_bits(value.to_bits(), valid_ptrs, ctx.pin_discoveries)
    {
        if !ctx.legacy_stats.is_null() {
            unsafe {
                (*ctx.legacy_stats).pinned_roots += 1;
                (*ctx.legacy_stats).pinned_bytes += bytes;
            }
        }
    }
}
