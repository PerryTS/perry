//! Runtime-resolved DPI APIs with Windows 7 fallbacks.
//!
//! Issue #303: Perry-compiled UI executables must start on Windows 7 SP1.
//! The two DPI primitives we want — `SetProcessDpiAwarenessContext` (Win10 1607)
//! and `GetDpiForSystem` (Win10 1607) — don't exist on Win7. Hard-importing
//! them via `extern "system" { fn ... }` or via the `windows` crate's
//! `windows::Win32::UI::HiDpi::*` re-exports causes link-time IAT entries that
//! the OS loader resolves BEFORE `main()` runs. On Win7 the loader fails the
//! process with "entry point not found in user32.dll" — there is no Rust code
//! we can write inside the binary that helps, because the failure happens
//! before Rust starts.
//!
//! Fix: resolve the Win10 functions lazily via `LoadLibraryW("user32.dll") +
//! GetProcAddress(...)`. If the symbol is present (Win10 1607+), call it.
//! Otherwise fall back through the version chain:
//!
//! | API used | Quality | Min Windows |
//! |---|---|---|
//! | `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` | Best (per-monitor v2) | Win10 1607 |
//! | `SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE)` | Per-monitor v1 | Win8.1 |
//! | `SetProcessDPIAware()` | System-wide DPI only | Vista |
//!
//! For DPI lookup:
//!
//! | API used | Min Windows |
//! |---|---|
//! | `GetDpiForSystem()` | Win10 1607 |
//! | `GetDC(NULL) + GetDeviceCaps(LOGPIXELSY)` | Win2000+ (always works) |
//!
//! Resolved fn pointers are cached in `AtomicPtr`s so we pay the LoadLibrary +
//! GetProcAddress cost exactly once per process. After the cache is warm the
//! cost is one atomic load + one indirect call per DPI query.
//!
//! See `docs/src/platforms/windows-7.md` for the user-facing story.

#![cfg(target_os = "windows")]

use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, AtomicU8, Ordering};

const STATE_UNRESOLVED: u8 = 0;
const STATE_AVAILABLE: u8 = 1;
const STATE_MISSING: u8 = 2;

static SET_DPI_AWARENESS_CONTEXT_STATE: AtomicU8 = AtomicU8::new(STATE_UNRESOLVED);
static SET_DPI_AWARENESS_CONTEXT_FN: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

static SET_PROCESS_DPI_AWARENESS_STATE: AtomicU8 = AtomicU8::new(STATE_UNRESOLVED);
static SET_PROCESS_DPI_AWARENESS_FN: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

static GET_DPI_FOR_SYSTEM_STATE: AtomicU8 = AtomicU8::new(STATE_UNRESOLVED);
static GET_DPI_FOR_SYSTEM_FN: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

#[link(name = "user32")]
extern "system" {
    fn LoadLibraryA(lpLibFileName: *const u8) -> *mut c_void;
    fn GetProcAddress(hModule: *mut c_void, lpProcName: *const u8) -> *mut c_void;
    fn SetProcessDPIAware() -> i32;
    fn GetDC(hWnd: *mut c_void) -> *mut c_void;
    fn ReleaseDC(hWnd: *mut c_void, hDC: *mut c_void) -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    fn GetDeviceCaps(hdc: *mut c_void, index: i32) -> i32;
}

/// `GetDeviceCaps` index for vertical DPI. Win2000+, dead reliable.
const LOGPIXELSY: i32 = 90;

static SHCORE_HANDLE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static SHCORE_RESOLVED: AtomicU8 = AtomicU8::new(STATE_UNRESOLVED);

unsafe fn user32() -> *mut c_void {
    LoadLibraryA(b"user32.dll\0".as_ptr())
}

unsafe fn shcore() -> *mut c_void {
    let cached = SHCORE_HANDLE.load(Ordering::Acquire);
    if !cached.is_null() {
        return cached;
    }
    if SHCORE_RESOLVED.load(Ordering::Acquire) == STATE_MISSING {
        return std::ptr::null_mut();
    }
    let h = LoadLibraryA(b"shcore.dll\0".as_ptr());
    if h.is_null() {
        SHCORE_RESOLVED.store(STATE_MISSING, Ordering::Release);
    } else {
        SHCORE_HANDLE.store(h, Ordering::Release);
        SHCORE_RESOLVED.store(STATE_AVAILABLE, Ordering::Release);
    }
    h
}

unsafe fn resolve_fn(
    module: *mut c_void,
    symbol: &[u8],
    state: &AtomicU8,
    slot: &AtomicPtr<c_void>,
) -> Option<*mut c_void> {
    match state.load(Ordering::Acquire) {
        STATE_AVAILABLE => Some(slot.load(Ordering::Acquire)),
        STATE_MISSING => None,
        _ => {
            if module.is_null() {
                state.store(STATE_MISSING, Ordering::Release);
                return None;
            }
            let proc = GetProcAddress(module, symbol.as_ptr());
            if proc.is_null() {
                state.store(STATE_MISSING, Ordering::Release);
                None
            } else {
                slot.store(proc, Ordering::Release);
                state.store(STATE_AVAILABLE, Ordering::Release);
                Some(proc)
            }
        }
    }
}

/// Best-effort DPI awareness opt-in. Tries Win10 1607 → Win8.1 → Vista in
/// order. DPI awareness is a hint, not a hard requirement — a process that
/// fails to opt in just runs at logical 96 DPI with the OS bitmap-scaling,
/// which looks fuzzy on hi-DPI displays but functions correctly. Never
/// panics, returns nothing.
pub fn set_process_dpi_awareness_compat() {
    unsafe {
        // Tier 1: Win10 1607+ — SetProcessDpiAwarenessContext in user32.
        let user32 = user32();
        if let Some(proc) = resolve_fn(
            user32,
            b"SetProcessDpiAwarenessContext\0",
            &SET_DPI_AWARENESS_CONTEXT_STATE,
            &SET_DPI_AWARENESS_CONTEXT_FN,
        ) {
            // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = -4 cast to handle.
            // The exact value is a stable Win10 ABI contract.
            type SetCtxFn = unsafe extern "system" fn(*mut c_void) -> i32;
            let f: SetCtxFn = std::mem::transmute(proc);
            let ctx_v2 = -4isize as *mut c_void;
            if f(ctx_v2) != 0 {
                return;
            }
            // If v2 was rejected (rare — only on builds where the symbol
            // exists but the v2 constant doesn't, e.g. Win10 1607 exact),
            // fall through to tier 2.
        }

        // Tier 2: Win8.1+ — SetProcessDpiAwareness in shcore.
        let shcore = shcore();
        if let Some(proc) = resolve_fn(
            shcore,
            b"SetProcessDpiAwareness\0",
            &SET_PROCESS_DPI_AWARENESS_STATE,
            &SET_PROCESS_DPI_AWARENESS_FN,
        ) {
            // PROCESS_PER_MONITOR_DPI_AWARE = 2
            type SetAwarenessFn = unsafe extern "system" fn(i32) -> i32;
            let f: SetAwarenessFn = std::mem::transmute(proc);
            // HRESULT: S_OK = 0. Anything else means failure or already-set
            // by a manifest entry — either way, don't fall through further.
            if f(2) == 0 {
                return;
            }
        }

        // Tier 3: Vista+ — SetProcessDPIAware in user32 (always present
        // since Vista, hard-imported above). System-wide DPI only.
        let _ = SetProcessDPIAware();
    }
}

/// System DPI in dots-per-inch. 96 = 100% scaling, 144 = 150%, 192 = 200%.
///
/// Tries `GetDpiForSystem` (Win10 1607+); falls back to `GetDC + GetDeviceCaps`
/// (Win2000+). Always returns at least 96. Never panics.
pub fn get_system_dpi_compat() -> u32 {
    unsafe {
        // Tier 1: Win10 1607+.
        let user32 = user32();
        if let Some(proc) = resolve_fn(
            user32,
            b"GetDpiForSystem\0",
            &GET_DPI_FOR_SYSTEM_STATE,
            &GET_DPI_FOR_SYSTEM_FN,
        ) {
            type GetDpiFn = unsafe extern "system" fn() -> u32;
            let f: GetDpiFn = std::mem::transmute(proc);
            let dpi = f();
            if dpi > 0 {
                return dpi;
            }
        }

        // Tier 2: Win2000+ — GetDC + GetDeviceCaps. Returns the same value
        // GetDpiForSystem would (system DPI), via the older mechanism.
        let hdc = GetDC(std::ptr::null_mut());
        if hdc.is_null() {
            return 96;
        }
        let dpi = GetDeviceCaps(hdc, LOGPIXELSY);
        let _ = ReleaseDC(std::ptr::null_mut(), hdc);
        if dpi > 0 {
            dpi as u32
        } else {
            96
        }
    }
}
