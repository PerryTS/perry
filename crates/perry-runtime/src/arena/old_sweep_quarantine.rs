//! Sweep quarantine (#11842): the non-moving sweeps' counterpart of
//! `PERRY_GC_PROTECT_FROMSPACE`.
//!
//! An object a sweep frees by mistake does not fault. Its bytes go back to the
//! allocator (a swept old hole is reused by the next same-size allocation, a
//! dead block is reset or released, a malloc object is `dealloc`ed), and a
//! holder that still points there reads an unrelated, newer object. The damage
//! shows up later and somewhere else: a wrong file, a lost field, a string
//! header with an absurd length.
//!
//! `PERRY_GC_PROTECT_OLD_SWEEP` (instrument builds only) stops that reuse:
//!
//! - every object a non-moving sweep frees (old or nursery) is recorded, its
//!   payload is filled with a poison word whose NaN-box is a pointer into
//!   unmapped memory, and its header is invalidated like an old hole;
//! - at `=1`, every whole page inside a freed old or malloc object is also made
//!   `PROT_NONE` (nursery pages are not: a copying minor resets whole nursery
//!   blocks, which `PERRY_GC_PROTECT_FROMSPACE` covers);
//! - no swept block is reset or released, no old hole is listed for reuse, a
//!   dead malloc object is never handed back to the system allocator, and a
//!   synchronous full visits every dead object instead of skipping dead blocks.
//!
//! The first use of a stale reference then faults. The installed reporter
//! names the freed object the fault address, or any register, points into,
//! and which sweep freed it, then prints a backtrace. `poison` fills and
//! records without protecting pages. Freed memory stays mapped for the life of
//! the process, so use it on bounded runs.

use super::quarantine::{mprotect_range, page_interior, FromSpaceProtection};
use std::sync::atomic::{AtomicU64, Ordering};

static SPANS_RETIRED: AtomicU64 = AtomicU64::new(0);
static BYTES_PROTECTED: AtomicU64 = AtomicU64::new(0);

/// Poison word: a NaN-boxed pointer to `0xDEAD_0000`, which is never mapped,
/// so a field read out of a freed object faults when it is followed.
const SWEPT_POISON_WORD: u64 = 0x7FFD_0000_DEAD_0000;

#[cfg(not(perry_gc_instruments))]
#[inline(always)]
pub(crate) fn old_sweep_protection_mode() -> FromSpaceProtection {
    FromSpaceProtection::Off
}

#[cfg(perry_gc_instruments)]
pub(crate) fn old_sweep_protection_mode() -> FromSpaceProtection {
    #[cfg(test)]
    if let Some(mode) = MODE_OVERRIDE.with(std::cell::Cell::get) {
        return mode;
    }
    use std::sync::OnceLock;
    static CACHED: OnceLock<FromSpaceProtection> = OnceLock::new();
    *crate::once_init::get_or_init(&CACHED, || {
        super::quarantine::parse_protection_mode(
            std::env::var("PERRY_GC_PROTECT_OLD_SWEEP").ok().as_deref(),
        )
    })
}

#[cfg(test)]
thread_local! {
    static MODE_OVERRIDE: std::cell::Cell<Option<FromSpaceProtection>> =
        const { std::cell::Cell::new(None) };
}

/// RAII test override for the mode, thread-local like the from-space one.
#[cfg(test)]
pub(crate) struct OldSweepProtectionGuard(Option<FromSpaceProtection>);

#[cfg(test)]
impl OldSweepProtectionGuard {
    pub(crate) fn set(mode: FromSpaceProtection) -> Self {
        Self(MODE_OVERRIDE.with(|cell| cell.replace(Some(mode))))
    }
}

#[cfg(test)]
impl Drop for OldSweepProtectionGuard {
    fn drop(&mut self) {
        MODE_OVERRIDE.with(|cell| cell.set(self.0));
    }
}

#[inline]
pub(crate) fn old_sweep_quarantine_enabled() -> bool {
    old_sweep_protection_mode() != FromSpaceProtection::Off
}

/// Where a retired object lived.
#[derive(Clone, Copy, Debug)]
pub(crate) enum RetiredKind {
    Nursery,
    Old,
    Malloc,
}

/// One retired object, kept so the fault reporter can say what used to live
/// at an address.
#[derive(Clone, Copy)]
struct RetiredSpan {
    header: usize,
    total_size: usize,
    obj_type: u8,
    kind: RetiredKind,
    /// The sweep that freed it, counted from 1.
    sweep_seq: u64,
    /// That sweep followed a budgeted (incremental) cycle.
    budgeted: bool,
}

static RETIRED: std::sync::Mutex<Vec<RetiredSpan>> = std::sync::Mutex::new(Vec::new());
static SWEEP_SEQ: AtomicU64 = AtomicU64::new(0);
static SWEEP_BUDGETED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Called once per sweep, so a report can say which sweep freed an object.
pub(crate) fn note_sweep_started(budgeted: bool) {
    if old_sweep_quarantine_enabled() {
        SWEEP_SEQ.fetch_add(1, Ordering::Relaxed);
        SWEEP_BUDGETED.store(budgeted, Ordering::Relaxed);
    }
}

/// Record, poison and (old/malloc, at `=1`) protect an object a sweep just
/// freed. The header is left in place, so heap walkers still see its size;
/// the caller invalidates it.
///
/// # Safety
/// `header` must head a dead object of `total_size` bytes that nothing may
/// legally touch again.
pub(crate) unsafe fn retire_swept_object(header: usize, total_size: usize, kind: RetiredKind) {
    let start = header + crate::gc::GC_HEADER_SIZE;
    let len = total_size.saturating_sub(crate::gc::GC_HEADER_SIZE);
    let mode = old_sweep_protection_mode();
    if SPANS_RETIRED.fetch_add(1, Ordering::Relaxed) == 0 {
        eprintln!(
            "[gc-sweep-quarantine] on: swept memory is poisoned{} and never reused",
            if mode == FromSpaceProtection::ProtectPages {
                " and protected"
            } else {
                ""
            }
        );
        install_fault_reporter();
    }
    let span = RetiredSpan {
        header,
        total_size,
        obj_type: (*(header as *const crate::gc::GcHeader)).obj_type,
        kind,
        sweep_seq: SWEEP_SEQ.load(Ordering::Relaxed),
        budgeted: SWEEP_BUDGETED.load(Ordering::Relaxed),
    };
    static THREAD_EXIT_HOOK: std::sync::Once = std::sync::Once::new();
    THREAD_EXIT_HOOK.call_once(|| {
        super::thread_exit::register_thread_exit_range_hook(release_retired_spans_in_freed_ranges)
    });
    if let Ok(mut spans) = RETIRED.lock() {
        spans.push(span);
    }
    let words = start as *mut u64;
    for i in 0..len / 8 {
        // GC_STORE_AUDIT(POINTER_FREE): diagnostic poison in a freed object.
        words.add(i).write(SWEPT_POISON_WORD);
    }
    if mode != FromSpaceProtection::ProtectPages || matches!(kind, RetiredKind::Nursery) {
        return;
    }
    if let Some((base, bytes)) = page_interior(start, len) {
        #[cfg(unix)]
        let prot_none = libc::PROT_NONE;
        #[cfg(not(unix))]
        let prot_none = 0i32;
        if mprotect_range(base, bytes, prot_none) {
            BYTES_PROTECTED.fetch_add(bytes as u64, Ordering::Relaxed);
        }
    }
}

/// Thread-exit range hook (#11471): the exiting thread's arena and
/// `gc_malloc` blocks go back to the allocator, quarantined objects included,
/// and another thread may reuse them. A span left over would make the
/// reporter call a fault on that newer object a use of freed memory.
fn release_retired_spans_in_freed_ranges(freed: &super::thread_exit::FreedRanges) {
    RETIRED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|span| !freed.contains(span.header));
}

#[cfg(not(unix))]
fn install_fault_reporter() {}

/// The SIGSEGV and SIGBUS actions installed before ours. The reporter hands
/// every fault back to them, so the stack-overflow guard and the other
/// instruments' reporters still see their own faults.
#[cfg(unix)]
static PREVIOUS_ACTIONS: std::sync::OnceLock<[libc::sigaction; 2]> = std::sync::OnceLock::new();

#[cfg(unix)]
fn install_fault_reporter() {
    // SAFETY: standard `sigaction` install with a `SA_SIGINFO` handler; the
    // previous actions are read into zeroed storage first.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = sweep_fault_handler as *const () as usize;
        action.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
        libc::sigemptyset(&mut action.sa_mask);
        let mut previous: [libc::sigaction; 2] = std::mem::zeroed();
        libc::sigaction(libc::SIGSEGV, &action, &mut previous[0]);
        libc::sigaction(libc::SIGBUS, &action, &mut previous[1]);
        let _ = PREVIOUS_ACTIONS.set(previous);
    }
}

fn find_span(spans: &[RetiredSpan], addr: usize) -> Option<RetiredSpan> {
    spans
        .iter()
        .rev()
        .find(|span| addr >= span.header && addr < span.header + span.total_size)
        .copied()
}

fn describe(what: &str, value: usize, span: RetiredSpan) {
    eprintln!(
        "  {what}={value:#x} is inside a freed object: header={:#x} +{} obj_type={} size={} \
         {:?} freed_by_sweep={} budgeted={} (sweeps so far: {})",
        span.header,
        value - span.header,
        span.obj_type,
        span.total_size,
        span.kind,
        span.sweep_seq,
        span.budgeted,
        SWEEP_SEQ.load(Ordering::Relaxed),
    );
}

/// Name the freed objects the fault address and the registers point into and
/// print a backtrace, then restore the previous handler so the instruction
/// faults again and is handled as it would have been. A fault that touches no
/// freed object is passed on silently. This is a debug path that runs once,
/// on the way down, so it may allocate.
#[cfg(unix)]
extern "C" fn sweep_fault_handler(
    signum: libc::c_int,
    info: *mut libc::siginfo_t,
    ctx: *mut libc::c_void,
) {
    let addr = if info.is_null() {
        0
    } else {
        // SAFETY: kernel-provided siginfo for SIGSEGV/SIGBUS.
        unsafe { (*info).si_addr() as usize }
    };
    if let Ok(spans) = RETIRED.try_lock() {
        let mut hits: Vec<(&str, usize, RetiredSpan)> = Vec::new();
        if let Some(span) = find_span(&spans, addr) {
            hits.push(("fault address", addr, span));
        }
        for (name, value) in registers(ctx) {
            if let Some(span) = find_span(&spans, value) {
                hits.push((name, value, span));
            }
        }
        if !hits.is_empty() {
            eprintln!("\n[gc-sweep-quarantine] signal {signum} at {addr:#x}");
            for (what, value, span) in hits {
                describe(what, value, span);
            }
            eprintln!("  backtrace:");
            super::quarantine::emit_native_backtrace();
        }
    }
    // Hand the fault back to whoever handled it before: the instruction faults
    // again and that handler (or the default action) deals with it.
    // SAFETY: restores an action `sigaction` itself returned, or the default.
    unsafe {
        let index = usize::from(signum == libc::SIGBUS);
        let mut action: libc::sigaction = match PREVIOUS_ACTIONS.get() {
            Some(previous) => previous[index],
            None => std::mem::zeroed(),
        };
        if PREVIOUS_ACTIONS.get().is_none() {
            action.sa_sigaction = libc::SIG_DFL;
            libc::sigemptyset(&mut action.sa_mask);
        }
        libc::sigaction(signum, &action, std::ptr::null_mut());
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn registers(ctx: *mut libc::c_void) -> Vec<(&'static str, usize)> {
    const NAMES: [&str; 17] = [
        "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15", "rdi", "rsi", "rbp", "rbx", "rdx",
        "rax", "rcx", "rsp", "rip",
    ];
    if ctx.is_null() {
        return Vec::new();
    }
    // SAFETY: `ctx` is the kernel-provided ucontext of the faulting thread.
    let gregs = unsafe { (*(ctx as *const libc::ucontext_t)).uc_mcontext.gregs };
    NAMES
        .iter()
        .enumerate()
        .map(|(i, &name)| (name, gregs[i] as usize))
        .collect()
}

#[cfg(all(unix, not(all(target_os = "linux", target_arch = "x86_64"))))]
fn registers(_ctx: *mut libc::c_void) -> Vec<(&'static str, usize)> {
    Vec::new()
}

/// `(objects retired, bytes protected)` since process start.
#[cfg(test)]
pub(crate) fn old_sweep_quarantine_stats() -> (u64, u64) {
    (
        SPANS_RETIRED.load(Ordering::Relaxed),
        BYTES_PROTECTED.load(Ordering::Relaxed),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span_at(header: usize) -> RetiredSpan {
        RetiredSpan {
            header,
            total_size: 64,
            obj_type: 0,
            kind: RetiredKind::Old,
            sweep_seq: 1,
            budgeted: false,
        }
    }

    /// Only the spans inside the exiting thread's blocks go; a span in memory
    /// that stays allocated still describes it.
    #[test]
    fn thread_exit_forgets_only_the_spans_in_freed_blocks() {
        // Addresses no real allocation in this process uses.
        let (freed_header, kept_header) = (0x7EAD_0000_1000usize, 0x7EAD_0010_1000usize);
        RETIRED
            .lock()
            .unwrap()
            .extend([span_at(freed_header), span_at(kept_header)]);
        release_retired_spans_in_freed_ranges(&super::super::thread_exit::FreedRanges::new(&[(
            freed_header,
            freed_header + 0x1000,
        )]));
        let mut spans = RETIRED.lock().unwrap();
        let left: Vec<usize> = spans
            .iter()
            .map(|span| span.header)
            .filter(|&header| header == freed_header || header == kept_header)
            .collect();
        spans.retain(|span| span.header != kept_header);
        assert_eq!(left, vec![kept_header]);
    }
}
