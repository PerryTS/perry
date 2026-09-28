//! Link-time feature installation for the runtime.
//!
//! The runtime's always-live hubs — the `globalThis` builder, the native-module
//! member lookup, the generic operators — must not name an optional subsystem
//! (the `eval` parser, Intl/ICU, Temporal, the Bun CLI utilities, …) directly.
//! A direct reference keeps that subsystem in every program linked against the
//! prebuilt full-feature `libperry_runtime.a`, which is what an installed perry
//! links when it has no workspace to rebuild from.
//!
//! Each optional feature instead fills [`Hook`] slots from its own
//! `js_runtime_install_<feature>` entry point, and a hub calls through the slot;
//! an empty slot takes the path the hub's `#[cfg(not(feature))]` branch takes
//! in a build without the feature. perry's link step generates an object whose
//! static constructor registers an installer through
//! [`js_runtime_register_feature_installer`]; [`crate::gc::js_gc_init`] runs it
//! before any user code. The installer names the program's runtime features on
//! a prebuilt-archive link and [`js_runtime_install_compiled`] otherwise (an
//! auto-optimized archive already holds exactly the needed features).
//!
//! perry-stdlib uses the same [`Hook`] for its own hubs
//! (`perry_stdlib::common::feature_hooks`).

use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};

/// One function-pointer slot. `F` must be a plain `fn` pointer type.
pub struct Hook<F: Copy> {
    bits: AtomicUsize,
    _f: PhantomData<F>,
}

impl<F: Copy> Hook<F> {
    pub const fn empty() -> Self {
        Self {
            bits: AtomicUsize::new(0),
            _f: PhantomData,
        }
    }

    #[inline]
    pub fn set(&self, f: F) {
        const { assert!(std::mem::size_of::<F>() == std::mem::size_of::<usize>()) };
        // SAFETY: `F` is a fn pointer of pointer size (asserted above).
        let bits: usize = unsafe { std::mem::transmute_copy(&f) };
        self.bits.store(bits, Ordering::Release);
    }

    #[inline]
    pub fn get(&self) -> Option<F> {
        #[cfg_attr(not(test), allow(unused_mut))]
        let mut bits = self.bits.load(Ordering::Acquire);
        // Unit tests reach hubs without the perry link step or `js_gc_init`,
        // so an empty slot installs everything compiled (the pre-hook
        // behavior) and is read again. Test builds only: a shipped archive
        // must not reference `js_runtime_install_compiled` from here.
        #[cfg(test)]
        if bits == 0 {
            install_compiled_for_tests();
            bits = self.bits.load(Ordering::Acquire);
        }
        if bits == 0 {
            None
        } else {
            // SAFETY: only `set` stores non-zero bits, and it stores an `F`.
            Some(unsafe { std::mem::transmute_copy(&bits) })
        }
    }
}

#[cfg(test)]
fn install_compiled_for_tests() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| js_runtime_install_compiled());
}

/// Run `$body` once per process, so installs are idempotent.
#[macro_export]
#[doc(hidden)]
macro_rules! perry_install_once {
    ($body:block) => {{
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| $body);
    }};
}

/// The installer the program's generated object registered.
static FEATURE_INSTALLER: Hook<extern "C" fn()> = Hook::empty();

/// Called from a static constructor in the object perry's link step generates.
/// Only stores the pointer: it runs before `main`; installing happens from
/// [`crate::gc::js_gc_init`].
#[no_mangle]
pub extern "C" fn js_runtime_register_feature_installer(installer: extern "C" fn()) {
    FEATURE_INSTALLER.set(installer);
}

/// Run the registered installer; called at the end of `js_gc_init`.
///
/// With no registration nothing optional is installed. This must not name
/// [`js_runtime_install_compiled`] outside tests: `js_gc_init` is live in every
/// program, so a fallback reference here would pin every feature again. The
/// crate's own unit tests keep the pre-hook behavior.
pub(crate) fn run_feature_installer() {
    if let Some(installer) = FEATURE_INSTALLER.get() {
        installer();
        return;
    }
    #[cfg(test)]
    js_runtime_install_compiled();
}

/// Install every optional runtime feature this archive was compiled with.
#[no_mangle]
pub extern "C" fn js_runtime_install_compiled() {
    #[cfg(feature = "dyn-eval")]
    js_runtime_install_dyn_eval();
    #[cfg(feature = "bun-cli-utils")]
    js_runtime_install_bun_cli_utils();
    #[cfg(feature = "intl-namespace")]
    js_runtime_install_intl_namespace();
    #[cfg(feature = "temporal")]
    js_runtime_install_temporal();
    #[cfg(feature = "intl-datetime")]
    js_runtime_install_intl_datetime();
    #[cfg(feature = "regex-engine")]
    js_runtime_install_regex_engine();
    #[cfg(feature = "url-engine")]
    js_runtime_install_url_engine();
}

#[cfg(feature = "dyn-eval")]
#[no_mangle]
pub extern "C" fn js_runtime_install_dyn_eval() {
    perry_install_once!({
        crate::object::install_dyn_eval();
        crate::dyn_eval_hooks::install();
    });
}

#[cfg(feature = "bun-cli-utils")]
#[no_mangle]
pub extern "C" fn js_runtime_install_bun_cli_utils() {
    perry_install_once!({
        crate::bun_compat::install_cli_utils();
    });
}

#[cfg(feature = "intl-namespace")]
#[no_mangle]
pub extern "C" fn js_runtime_install_intl_namespace() {
    perry_install_once!({
        crate::intl::install_intl_namespace_feature();
    });
}

#[cfg(feature = "temporal")]
#[no_mangle]
pub extern "C" fn js_runtime_install_temporal() {
    perry_install_once!({
        crate::temporal::hooked::install();
    });
}

#[cfg(feature = "intl-datetime")]
#[no_mangle]
pub extern "C" fn js_runtime_install_intl_datetime() {
    perry_install_once!({
        crate::date::install_compiled_tzdb();
    });
}

#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_runtime_install_regex_engine() {
    perry_install_once!({
        crate::regex::install_iterator_hooks();
    });
}

#[cfg(feature = "url-engine")]
#[no_mangle]
pub extern "C" fn js_runtime_install_url_engine() {
    perry_install_once!({
        crate::url::install_engine();
    });
}
