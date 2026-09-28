//! Link-time feature installation.
//!
//! The always-live stdlib hubs (`js_handle_method_dispatch` and its property
//! siblings, the async pump, `js_stdlib_init_dispatch`) must not name an
//! optional subsystem directly. A direct reference keeps that subsystem alive
//! in every program linked against the prebuilt full-feature archive — the one
//! an installed `perry` links when it has no workspace to rebuild from — so a
//! program that never touches sqlite or WebCrypto still carried both.
//!
//! Each optional feature instead fills [`Hook`] slots from its own
//! `js_stdlib_install_<feature>` entry point. The hubs call through the slots,
//! and a slot nobody filled is a null check, so an uninstalled feature's code
//! is unreferenced and the linker drops it. The compiler decides which install
//! entry points to call: perry's link step generates an object whose static
//! constructor registers an installer through
//! [`js_stdlib_register_feature_installer`], and `js_stdlib_init_dispatch` runs
//! it. The installer calls the program's required features on a prebuilt link,
//! or [`js_stdlib_install_compiled`] on an auto-optimized link, where the
//! archive was already built with exactly the needed features.
//!
//! An install covers only its own feature. The compiler expands the program's
//! features through this crate's `[features]` table before choosing installs,
//! so `web-fetch` also installs `bundled-streams` and `turnloop-http-client`
//! without this module restating Cargo's implications. The installable names
//! are listed in perry's `stdlib_installs.rs`; a test there keeps that list
//! and the `js_stdlib_install_*` functions below in step.
//!
//! Slots are fixed per hub position, so the dispatch order is the one the hubs
//! spell out regardless of the order features are installed in, and every
//! install is idempotent.

use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};

/// One function-pointer slot. `F` must be a plain `fn` pointer type.
pub(crate) struct Hook<F: Copy> {
    bits: AtomicUsize,
    _f: PhantomData<F>,
}

impl<F: Copy> Hook<F> {
    pub(crate) const fn empty() -> Self {
        Self {
            bits: AtomicUsize::new(0),
            _f: PhantomData,
        }
    }

    #[inline]
    pub(crate) fn set(&self, f: F) {
        const { assert!(std::mem::size_of::<F>() == std::mem::size_of::<usize>()) };
        // SAFETY: `F` is a fn pointer of pointer size (asserted above).
        let bits: usize = unsafe { std::mem::transmute_copy(&f) };
        self.bits.store(bits, Ordering::Release);
    }

    #[inline]
    pub(crate) fn get(&self) -> Option<F> {
        let bits = self.bits.load(Ordering::Acquire);
        if bits == 0 {
            None
        } else {
            // SAFETY: only `set` stores non-zero bits, and it stores an `F`.
            Some(unsafe { std::mem::transmute_copy(&bits) })
        }
    }
}

/// A hub arm that may claim a method call on a native handle.
pub(crate) type MethodArm = unsafe fn(i64, &str, &[f64]) -> Option<f64>;
/// A method arm that runs before the hub copies the name and arguments.
pub(crate) type RawMethodArm = unsafe fn(i64, *const u8, usize, *const f64, usize) -> Option<f64>;
/// A hub arm that may claim a property read on a native handle.
pub(crate) type PropertyArm = unsafe fn(i64, &str) -> Option<f64>;
/// A hub arm that may claim a property write; `true` means claimed.
pub(crate) type PropertySetArm = unsafe fn(i64, &str, f64) -> bool;
/// A hub arm answering a whole-handle query (own keys, prototype).
pub(crate) type HandleArm = unsafe fn(i64) -> Option<f64>;

/// Run `$body` once per process. Every install goes through this, so calling
/// an install twice (directly and through an implying feature) is harmless.
// Unused only in a build with no optional feature at all.
#[allow(unused_macros)]
macro_rules! install_once {
    ($body:block) => {{
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| $body);
    }};
}

/// The installer the program's generated object registered (see
/// [`js_stdlib_register_feature_installer`]).
static FEATURE_INSTALLER: Hook<extern "C" fn()> = Hook::empty();

/// Called from a static constructor in the object perry's link step generates
/// for every program that links this archive. Only stores the pointer: it runs
/// before `main`, and installing happens later from `js_stdlib_init_dispatch`.
#[no_mangle]
pub extern "C" fn js_stdlib_register_feature_installer(installer: extern "C" fn()) {
    FEATURE_INSTALLER.set(installer);
}

/// Run the registered installer. `js_stdlib_init_dispatch` calls this after
/// its core registrations, on whichever path first initializes the stdlib.
///
/// With no registration nothing optional is installed. This function must not
/// name [`js_stdlib_install_compiled`] outside tests: `js_stdlib_init_dispatch`
/// is live in every program, so a fallback reference here would pin every
/// feature again. perry's link step always registers an installer; a Rust
/// binary linking this crate directly calls `js_stdlib_install_compiled`
/// itself. This crate's own unit tests keep the pre-hook behavior.
pub(crate) fn run_feature_installer() {
    if let Some(installer) = FEATURE_INSTALLER.get() {
        installer();
        return;
    }
    #[cfg(test)]
    js_stdlib_install_compiled();
}

/// Install every optional feature this archive was compiled with. An
/// auto-optimized link calls this: its archive holds exactly the program's
/// features, so installing all of them is the pre-hook behavior. A prebuilt
/// link calls it too when the program has dynamic code the compiler cannot see
/// through.
#[no_mangle]
pub extern "C" fn js_stdlib_install_compiled() {
    #[cfg(feature = "bundled-streams")]
    js_stdlib_install_bundled_streams();
    #[cfg(any(feature = "bundled-events", feature = "external-events-construct"))]
    js_stdlib_install_events();
    #[cfg(feature = "bundled-nodemailer")]
    js_stdlib_install_bundled_nodemailer();
    #[cfg(feature = "database-sqlite")]
    js_stdlib_install_database_sqlite();
    #[cfg(feature = "crypto")]
    js_stdlib_install_crypto();
    #[cfg(feature = "tls-runtime")]
    js_stdlib_install_tls_runtime();
    #[cfg(feature = "compression-gzip")]
    js_stdlib_install_compression_gzip();
    #[cfg(feature = "external-zlib-pump")]
    js_stdlib_install_external_zlib_pump();
    #[cfg(feature = "external-http-client-pump")]
    js_stdlib_install_external_http_client_pump();
    #[cfg(feature = "external-http-server-pump")]
    js_stdlib_install_external_http_server_pump();
    #[cfg(feature = "external-net-pump")]
    js_stdlib_install_external_net_pump();
    #[cfg(feature = "web-fetch")]
    js_stdlib_install_web_fetch();
    #[cfg(feature = "turnloop-http-client")]
    js_stdlib_install_turnloop_http_client();
    #[cfg(feature = "turnloop-smtp-client")]
    js_stdlib_install_turnloop_smtp_client();
}

#[cfg(feature = "bundled-streams")]
#[no_mangle]
pub extern "C" fn js_stdlib_install_bundled_streams() {
    install_once!({
        super::dispatch::install_streams();
    });
}

#[cfg(any(feature = "bundled-events", feature = "external-events-construct"))]
#[no_mangle]
pub extern "C" fn js_stdlib_install_events() {
    install_once!({
        super::dispatch::install_events();
    });
}

#[cfg(feature = "bundled-nodemailer")]
#[no_mangle]
pub extern "C" fn js_stdlib_install_bundled_nodemailer() {
    install_once!({
        super::dispatch::install_nodemailer();
    });
}

#[cfg(feature = "database-sqlite")]
#[no_mangle]
pub extern "C" fn js_stdlib_install_database_sqlite() {
    install_once!({
        super::dispatch::install_sqlite();
    });
}

#[cfg(feature = "crypto")]
#[no_mangle]
pub extern "C" fn js_stdlib_install_crypto() {
    install_once!({
        super::dispatch::install_crypto();
        super::async_bridge::install_crypto_pump();
    });
}

#[cfg(feature = "tls-runtime")]
#[no_mangle]
pub extern "C" fn js_stdlib_install_tls_runtime() {
    install_once!({
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
            super::dispatch::install_tls();
            super::async_bridge::install_tls_pump();
        }
    });
}

#[cfg(feature = "compression-gzip")]
#[no_mangle]
pub extern "C" fn js_stdlib_install_compression_gzip() {
    install_once!({
        super::dispatch::install_zlib();
        super::async_bridge::install_zlib_pump();
    });
}

#[cfg(feature = "external-zlib-pump")]
#[no_mangle]
pub extern "C" fn js_stdlib_install_external_zlib_pump() {
    install_once!({
        super::dispatch::install_external_zlib();
    });
}

#[cfg(feature = "external-http-client-pump")]
#[no_mangle]
pub extern "C" fn js_stdlib_install_external_http_client_pump() {
    install_once!({
        super::dispatch::install_external_http_client();
    });
}

#[cfg(feature = "external-http-server-pump")]
#[no_mangle]
pub extern "C" fn js_stdlib_install_external_http_server_pump() {
    install_once!({
        super::dispatch::install_external_http_server();
    });
}

#[cfg(feature = "external-net-pump")]
#[no_mangle]
pub extern "C" fn js_stdlib_install_external_net_pump() {
    install_once!({
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        super::dispatch::install_external_net();
    });
}

#[cfg(feature = "web-fetch")]
#[no_mangle]
pub extern "C" fn js_stdlib_install_web_fetch() {
    install_once!({
        super::dispatch::install_fetch();
    });
}

#[cfg(feature = "turnloop-http-client")]
#[no_mangle]
pub extern "C" fn js_stdlib_install_turnloop_http_client() {
    install_once!({
        super::async_bridge::install_turnloop_http_client_active();
    });
}

#[cfg(feature = "turnloop-smtp-client")]
#[no_mangle]
pub extern "C" fn js_stdlib_install_turnloop_smtp_client() {
    install_once!({
        super::async_bridge::install_turnloop_smtp_active();
    });
}
