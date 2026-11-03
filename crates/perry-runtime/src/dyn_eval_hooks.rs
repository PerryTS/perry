//! The script evaluator's (`crate::dyn_eval`) touch points in always-live code.
//!
//! The `Function` constructor, dynamic `import()` of a JavaScript `data:` URL,
//! and the GC (a root scanner, the move hook, the dead-owner prune) reach the
//! interpreter. Those callers are live in
//! every program, so they must not name `crate::dyn_eval` directly, or the
//! prebuilt full-feature runtime keeps the interpreter and the JS parser behind
//! it in every binary. They call these forwarders, which reach it only through
//! slots the `dyn-eval` install fills (see `crate::feature_hooks`).
//!
//! The compiler-selected install runs before user code and registers the
//! evaluator's roots for the current agent. Later agents register them through
//! the installed root callback in `gc_init`. Empty slots provide the absence
//! behavior: no function, no scan, no move bookkeeping.

use crate::feature_hooks::Hook;

static FUNCTION_FROM_STRINGS: Hook<fn(&[String]) -> f64> = Hook::empty();
static DATA_URL_IMPORT: Hook<fn(&str) -> Option<f64>> = Hook::empty();

/// `new Function(...params, body)` from strings; `None` without the evaluator.
pub(crate) fn function_from_strings(args: &[String]) -> Option<f64> {
    FUNCTION_FROM_STRINGS.get().map(|f| f(args))
}

/// Dynamic `import()` of a JavaScript `data:` URL; `None` without the
/// evaluator, which leaves the loader's "cannot find module" path.
pub(crate) fn dynamic_import_data_url(specifier: &str) -> Option<f64> {
    DATA_URL_IMPORT.get().and_then(|f| f(specifier))
}

/// The `dyn-eval` install: the only writer of these slots.
#[cfg(feature = "dyn-eval")]
pub(crate) fn install() {
    crate::dyn_eval::install_function_owner_storage();
    fn register_roots() {
        crate::gc::gc_register_mutable_root_scanner_named_once(
            "crate::dyn_eval::scan_dyn_eval_roots_mut",
            crate::dyn_eval::scan_dyn_eval_roots_mut,
        );
    }
    crate::dyn_eval_hook_roots::install(register_roots);
    FUNCTION_FROM_STRINGS.set(crate::dyn_eval::dyn_function_from_strings);
    DATA_URL_IMPORT.set(crate::module_require::dynamic_import_javascript_data_url);
}

pub(crate) use crate::interp_proto_guard::note_proto_write;

#[cfg(feature = "dyn-eval")]
pub(crate) use crate::interp_proto_guard::{interp_active_enter, interp_active_leave};
