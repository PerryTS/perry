//! WinUI 3 / Fluent backend internals — issue #4680.
//!
//! This module is where the Win32 widget creation is progressively replaced by
//! `Microsoft.UI.Xaml` controls. It is empty of real XAML today (scaffold);
//! see the crate-level docs for the incremental plan. Each future widget gets a
//! submodule here that drives the corresponding XAML control and is wired into
//! the dispatch path in place of the `perry-ui-windows` Win32 path.

/// Windows App SDK bootstrap (#4680 step 2).
///
/// A WinUI 3 / unpackaged app must initialize the Windows App SDK runtime
/// before any `Microsoft.UI.Xaml` type is constructed. The runtime ships the
/// bootstrapper entry points (`MddBootstrapInitialize2` /
/// `MddBootstrapInitialize`) in `Microsoft.WindowsAppRuntime.Bootstrap.dll`.
///
/// # Why dynamic loading (not a link dependency)
///
/// Perry's defining constraint is the single self-contained `.exe`. Linking
/// `Microsoft.WindowsAppRuntime.Bootstrap.lib` would make *every*
/// `windows-winui` binary hard-require the Windows App SDK at load time — the
/// process would fail to start on a machine that doesn't have it, even though
/// the scaffold can fall back to the Win32 backend and run fine. So instead of
/// a link-time import we resolve the bootstrapper at runtime with
/// `LoadLibraryW` + `GetProcAddress`. If the DLL isn't present (no Windows App
/// SDK installed), [`initialize`] reports [`InitStatus::RuntimeMissing`] and
/// the caller falls back to Win32 rather than crashing. This keeps the binary
/// dependency-free; the SDK is consumed only when the host actually has it.
///
/// The result is cached after the first call: the runtime is process-wide and
/// initialized at most once, so repeated [`initialize`] calls are cheap and
/// return a stable answer.
pub mod bootstrap {
    use std::sync::atomic::{AtomicU8, Ordering};

    /// Outcome of attempting to initialize the Windows App SDK runtime.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum InitStatus {
        /// The Windows App SDK runtime is present and initialized — the WinUI
        /// (XAML) rendering path is usable.
        Ready,
        /// The Windows App SDK runtime is not installed (or failed to
        /// initialize); the caller should fall back to the Win32 backend
        /// rather than crash.
        RuntimeMissing,
    }

    // Cached outcome of the one-time initialize() probe. The runtime is
    // process-wide, so we resolve + bootstrap it at most once and memoize the
    // verdict. 0 = not yet attempted, 1 = Ready, 2 = RuntimeMissing.
    const CACHE_UNINIT: u8 = 0;
    const CACHE_READY: u8 = 1;
    const CACHE_MISSING: u8 = 2;
    static CACHED: AtomicU8 = AtomicU8::new(CACHE_UNINIT);

    /// Initialize the Windows App SDK runtime, returning whether the WinUI
    /// (XAML) path is usable. On Windows this dynamically loads the
    /// bootstrapper and calls `MddBootstrapInitialize2` (falling back to
    /// `MddBootstrapInitialize`); if the runtime is absent or initialization
    /// fails it returns [`InitStatus::RuntimeMissing`] so the caller can fall
    /// back to Win32. Off Windows it is always [`InitStatus::RuntimeMissing`].
    ///
    /// The result is cached: subsequent calls return the first verdict without
    /// re-loading the DLL. This is the #4680 step-2 deliverable; the XAML
    /// widget mapping (step 3) consults this before constructing any
    /// `Microsoft.UI.Xaml` object.
    pub fn initialize() -> InitStatus {
        match CACHED.load(Ordering::Acquire) {
            CACHE_READY => return InitStatus::Ready,
            CACHE_MISSING => return InitStatus::RuntimeMissing,
            _ => {}
        }
        let status = init_uncached();
        CACHED.store(
            match status {
                InitStatus::Ready => CACHE_READY,
                InitStatus::RuntimeMissing => CACHE_MISSING,
            },
            Ordering::Release,
        );
        status
    }

    #[cfg(target_os = "windows")]
    fn init_uncached() -> InitStatus {
        windows_impl::bootstrap_initialize()
    }

    #[cfg(not(target_os = "windows"))]
    fn init_uncached() -> InitStatus {
        // There is no Windows App SDK off Windows. The crate still compiles for
        // host tooling (the workspace builds it on every platform); callers get
        // RuntimeMissing and fall back to the Win32 path.
        InitStatus::RuntimeMissing
    }

    #[cfg(target_os = "windows")]
    pub(super) fn last_init_detail() -> i32 {
        windows_impl::last_detail()
    }

    #[cfg(not(target_os = "windows"))]
    pub(super) fn last_init_detail() -> i32 {
        windows_impl_detail_off_windows()
    }

    #[cfg(not(target_os = "windows"))]
    fn windows_impl_detail_off_windows() -> i32 {
        DETAIL_NOT_WINDOWS
    }

    /// Human-readable form of a [`last_init_detail`] code, for `PERRY_WINUI_DIAG`.
    pub(super) fn describe_init_detail(detail: i32) -> String {
        match detail {
            DETAIL_NOT_ATTEMPTED => "not attempted".to_string(),
            DETAIL_DLL_MISSING => {
                "bootstrap DLL not found (ship Microsoft.WindowsAppRuntime.Bootstrap.dll next to the exe)".to_string()
            }
            DETAIL_NO_ENTRYPOINT => "bootstrap DLL has no MddBootstrapInitialize entry point".to_string(),
            DETAIL_SUCCESS => "ready".to_string(),
            #[cfg(not(target_os = "windows"))]
            DETAIL_NOT_WINDOWS => "not windows".to_string(),
            // Anything else is the raw HRESULT from MddBootstrapInitialize*.
            hr => format!("MddBootstrapInitialize failed, HRESULT 0x{:08X}", hr as u32),
        }
    }

    /// Sentinel "detail" codes returned by [`last_init_detail`] that are not real
    /// HRESULTs (which are never these tiny positive values for our paths). Any
    /// other value is the raw HRESULT from the bootstrapper call.
    pub(super) const DETAIL_NOT_ATTEMPTED: i32 = 0;
    pub(super) const DETAIL_DLL_MISSING: i32 = 1;
    pub(super) const DETAIL_NO_ENTRYPOINT: i32 = 2;
    pub(super) const DETAIL_SUCCESS: i32 = 3;
    #[cfg(not(target_os = "windows"))]
    pub(super) const DETAIL_NOT_WINDOWS: i32 = 4;

    #[cfg(target_os = "windows")]
    mod windows_impl {
        use super::{
            InitStatus, DETAIL_DLL_MISSING, DETAIL_NOT_ATTEMPTED, DETAIL_NO_ENTRYPOINT,
            DETAIL_SUCCESS,
        };
        use std::sync::atomic::{AtomicI32, Ordering};

        // Last bootstrap outcome detail: a sentinel (see DETAIL_*) or, for a
        // failed init call, the raw HRESULT. Surfaced via PERRY_WINUI_DIAG so a
        // RuntimeMissing verdict can be diagnosed (e.g. "runtime not installed"
        // vs "wrong version requested").
        static LAST_DETAIL: AtomicI32 = AtomicI32::new(DETAIL_NOT_ATTEMPTED);

        pub(super) fn last_detail() -> i32 {
            LAST_DETAIL.load(Ordering::Relaxed)
        }

        type HModule = *mut core::ffi::c_void;
        type FarProc = *const core::ffi::c_void;

        extern "system" {
            fn LoadLibraryW(name: *const u16) -> HModule;
            fn GetProcAddress(module: HModule, name: *const u8) -> FarProc;
            fn FreeLibrary(module: HModule) -> i32;
        }

        // Bootstrapper entry points (`MddBootstrap.h`). `PACKAGE_VERSION` is a
        // union over a single `UINT64`, so on x64 it is ABI-identical to a
        // by-value `u64`; `versionTag` is a `PCWSTR`; `options` is an `enum`
        // (`int`). The `2`-suffixed variant (Windows App SDK 1.2+) takes the
        // extra `options` argument; the original is the fallback for older
        // bootstrappers.
        type PfnInitialize2 = unsafe extern "system" fn(u32, *const u16, u64, i32) -> i32;
        type PfnInitialize = unsafe extern "system" fn(u32, *const u16, u64) -> i32;

        /// `MddBootstrapInitializeOptions_None`.
        const MDD_BOOTSTRAP_OPTIONS_NONE: i32 = 0;

        /// Packed `major << 16 | minor` Windows App SDK release the binary was
        /// built against. Defaults to 1.6 (the current servicing baseline) and
        /// is overridable at runtime with `PERRY_WINAPPSDK_VERSION="major.minor"`
        /// so a host with a different SDK can be targeted without a rebuild.
        fn target_major_minor() -> u32 {
            const DEFAULT_MAJOR: u32 = 1;
            const DEFAULT_MINOR: u32 = 6;
            if let Ok(raw) = std::env::var("PERRY_WINAPPSDK_VERSION") {
                if let Some((maj, min)) = raw.split_once('.') {
                    if let (Ok(maj), Ok(min)) =
                        (maj.trim().parse::<u16>(), min.trim().parse::<u16>())
                    {
                        return ((maj as u32) << 16) | (min as u32);
                    }
                }
            }
            (DEFAULT_MAJOR << 16) | DEFAULT_MINOR
        }

        fn wide_nul(s: &str) -> Vec<u16> {
            s.encode_utf16().chain(std::iter::once(0)).collect()
        }

        pub fn bootstrap_initialize() -> InitStatus {
            let dll = wide_nul("Microsoft.WindowsAppRuntime.Bootstrap.dll");
            // SAFETY: `dll` is a NUL-terminated UTF-16 buffer that outlives the
            // call. A missing DLL returns NULL (no Windows App SDK) — we never
            // dereference the handle in that case.
            let module = unsafe { LoadLibraryW(dll.as_ptr()) };
            if module.is_null() {
                LAST_DETAIL.store(DETAIL_DLL_MISSING, Ordering::Relaxed);
                return InitStatus::RuntimeMissing;
            }

            let major_minor = target_major_minor();
            // Stable release channel uses an empty version tag; minimum package
            // version 0 accepts any installed framework at/above major_minor.
            let version_tag = wide_nul("");
            let min_version: u64 = 0;

            // SAFETY: the function pointers come from this freshly-loaded module
            // and are transmuted to the documented `MddBootstrap.h` signatures.
            // Both pointer arguments outlive the synchronous call. If neither
            // entry point resolves the DLL is not a usable bootstrapper, so we
            // free it and report the runtime missing.
            let hr = unsafe {
                let init2 = GetProcAddress(module, b"MddBootstrapInitialize2\0".as_ptr());
                if !init2.is_null() {
                    let f: PfnInitialize2 = core::mem::transmute(init2);
                    f(
                        major_minor,
                        version_tag.as_ptr(),
                        min_version,
                        MDD_BOOTSTRAP_OPTIONS_NONE,
                    )
                } else {
                    let init1 = GetProcAddress(module, b"MddBootstrapInitialize\0".as_ptr());
                    if init1.is_null() {
                        FreeLibrary(module);
                        LAST_DETAIL.store(DETAIL_NO_ENTRYPOINT, Ordering::Relaxed);
                        return InitStatus::RuntimeMissing;
                    }
                    let f: PfnInitialize = core::mem::transmute(init1);
                    f(major_minor, version_tag.as_ptr(), min_version)
                }
            };

            if hr == 0 {
                // Success: the runtime must stay mapped for the process
                // lifetime, so we deliberately do NOT FreeLibrary here.
                LAST_DETAIL.store(DETAIL_SUCCESS, Ordering::Relaxed);
                InitStatus::Ready
            } else {
                // S_OK is the only success; any failure HRESULT (e.g. the
                // framework not being present at the requested version) means
                // we fall back to Win32. Release the handle we won't use.
                // SAFETY: `module` is the handle we just loaded.
                unsafe { FreeLibrary(module) };
                LAST_DETAIL.store(hr, Ordering::Relaxed);
                InitStatus::RuntimeMissing
            }
        }
    }
}

/// Which rendering backend a `windows-winui` process resolves to (#4680 step 3
/// seam).
///
/// The Fluent (WinUI 3 / `Microsoft.UI.Xaml`) path is only usable when the
/// Windows App SDK runtime initialized — otherwise the backend falls back to
/// the re-exported Win32 path so the app still renders. This module is the
/// single decision point the per-widget XAML dispatch reads: each XAML widget
/// (landing incrementally) checks [`backend::active`] and constructs an
/// `Microsoft.UI.Xaml` control on [`RenderBackend::Fluent`], else delegates to
/// the existing `perry-ui-windows` Win32 constructor.
pub mod backend {
    use super::bootstrap::{self, InitStatus};

    /// The effective render backend for this process.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum RenderBackend {
        /// WinUI 3 / Fluent (`Microsoft.UI.Xaml`) — chosen when the Windows
        /// App SDK runtime is `Ready`.
        Fluent,
        /// Win32 / GDI (the re-exported `perry-ui-windows` backend) — chosen
        /// whenever the Windows App SDK runtime is unavailable.
        Win32,
    }

    impl RenderBackend {
        /// Stable lowercase identifier, used in diagnostics.
        pub fn as_str(self) -> &'static str {
            match self {
                RenderBackend::Fluent => "fluent",
                RenderBackend::Win32 => "win32",
            }
        }
    }

    /// The render backend this process will use, derived from the Windows App
    /// SDK bootstrap probe ([`bootstrap::initialize`]) and memoized by it:
    /// [`RenderBackend::Fluent`] iff the runtime is `Ready`, otherwise
    /// [`RenderBackend::Win32`]. Cheap and stable across calls.
    pub fn active() -> RenderBackend {
        match bootstrap::initialize() {
            InitStatus::Ready => RenderBackend::Fluent,
            InitStatus::RuntimeMissing => RenderBackend::Win32,
        }
    }
}

/// Process-startup probe (#4680 step 3 seam).
///
/// Registered as a CRT static initializer (`.CRT$XCU`) so it runs at process
/// start for *any* binary that links this staticlib — i.e. exactly the
/// `--target windows-winui` builds, and never the default `--target windows`
/// builds (which don't link this crate). It resolves the render backend up
/// front (probing the Windows App SDK once) so the first widget construction
/// reads an already-decided answer, and emits a one-line backend diagnostic
/// when `PERRY_WINUI_DIAG` is set. It must never panic — it runs before `main`.
#[cfg(target_os = "windows")]
#[used]
#[link_section = ".CRT$XCU"]
static PERRY_WINUI_STARTUP: extern "C" fn() = perry_winui_startup;

#[cfg(target_os = "windows")]
extern "C" fn perry_winui_startup() {
    let backend = backend::active();
    if std::env::var_os("PERRY_WINUI_DIAG").is_some() {
        use std::io::Write;
        let detail = bootstrap::last_init_detail();
        // Best-effort: a failed write in a static initializer is ignored.
        let _ = std::io::stderr().write_all(
            format!(
                "[perry-winui] render backend: {} (bootstrap detail: {})\n",
                backend.as_str(),
                bootstrap::describe_init_detail(detail),
            )
            .as_bytes(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::backend::{active, RenderBackend};
    use super::bootstrap::{initialize, InitStatus};

    #[test]
    fn initialize_is_total_and_idempotent() {
        // initialize() must never panic and must return a stable, cached
        // verdict regardless of whether the Windows App SDK is installed on the
        // test host. (On CI without the SDK, that verdict is RuntimeMissing.)
        let first = initialize();
        let second = initialize();
        assert_eq!(
            first, second,
            "cached bootstrap verdict must be stable across calls"
        );
        assert!(matches!(
            first,
            InitStatus::Ready | InitStatus::RuntimeMissing
        ));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn missing_off_windows() {
        // There is no Windows App SDK off Windows, so the verdict is always
        // RuntimeMissing — the caller falls back to Win32.
        assert_eq!(initialize(), InitStatus::RuntimeMissing);
    }

    #[test]
    fn active_backend_matches_bootstrap_verdict() {
        // The backend seam must mirror the bootstrap probe exactly and be
        // stable across calls.
        let expected = match initialize() {
            InitStatus::Ready => RenderBackend::Fluent,
            InitStatus::RuntimeMissing => RenderBackend::Win32,
        };
        assert_eq!(active(), expected);
        assert_eq!(active(), active(), "backend choice must be stable");
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn backend_is_win32_off_windows() {
        // No Windows App SDK off Windows → always the Win32 fallback.
        assert_eq!(active(), RenderBackend::Win32);
    }

    #[test]
    fn describe_detail_formats_known_codes_and_raw_hresult() {
        use super::bootstrap::{
            describe_init_detail, DETAIL_DLL_MISSING, DETAIL_NOT_ATTEMPTED, DETAIL_SUCCESS,
        };
        assert_eq!(describe_init_detail(DETAIL_NOT_ATTEMPTED), "not attempted");
        assert_eq!(describe_init_detail(DETAIL_SUCCESS), "ready");
        assert!(describe_init_detail(DETAIL_DLL_MISSING).contains("Bootstrap.dll"));
        // A raw bootstrapper HRESULT renders as 0x-prefixed hex — e.g. the
        // 0x80670016 "no matching runtime/DDLM" failure seen when only the
        // framework package (not the DDLM) is registered.
        let s = describe_init_detail(0x8067_0016u32 as i32);
        assert!(s.contains("0x80670016"), "got: {s}");
    }
}
