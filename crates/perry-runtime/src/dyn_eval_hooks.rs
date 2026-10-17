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
//! The install runs from `js_gc_init`, before any user code, so the
//! interpreter never runs with a slot empty. An empty slot answers what a build
//! without `dyn-eval` answers: no function, no scan, no move bookkeeping.

use crate::feature_hooks::Hook;

static FUNCTION_FROM_STRINGS: Hook<fn(&[String]) -> f64> = Hook::empty();
static SCAN_ROOTS: Hook<fn(&mut crate::gc::RuntimeRootVisitor<'_>)> = Hook::empty();
static DATA_URL_IMPORT: Hook<fn(&str) -> Option<f64>> = Hook::empty();

/// `new Function(...params, body)` from strings; `None` without the evaluator.
pub(crate) fn function_from_strings(args: &[String]) -> Option<f64> {
    FUNCTION_FROM_STRINGS.get().map(|f| f(args))
}

/// Root scanner for the interpreter's rooted value stack (#6559). Registered
/// unconditionally in `gc_init`; scans only once the evaluator is installed.
pub fn scan_dyn_eval_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    if let Some(scan) = SCAN_ROOTS.get() {
        scan(visitor);
    }
}

/// Dynamic `import()` of a JavaScript `data:` URL; `None` without the
/// evaluator, which leaves the loader's "cannot find module" path.
pub(crate) fn dynamic_import_data_url(specifier: &str) -> Option<f64> {
    DATA_URL_IMPORT.get().and_then(|f| f(specifier))
}

/// The `dyn-eval` install: the only writer of these slots.
#[cfg(feature = "dyn-eval")]
pub(crate) fn install() {
    FUNCTION_FROM_STRINGS.set(crate::dyn_eval::dyn_function_from_strings);
    SCAN_ROOTS.set(crate::dyn_eval::scan_dyn_eval_roots_mut);
    DATA_URL_IMPORT.set(crate::module_require::dynamic_import_javascript_data_url);
}

/// Decision 150: interpreted code wrote a prototype the whole-program facts
/// cannot see it write. Set-only, process-wide, `0` until then.
///
/// The facts (perry-wpfacts) model interpreted code as the ⊤ summary node:
/// every value it can receive already escapes. What that model cannot see is
/// interpreted code writing a PROTOTYPE that program objects inherit from
/// (`Object.prototype`, `Array.prototype` …, or any object marked as a
/// prototype): an accessor or method it installs there runs with the
/// receiver of every lookup that reaches it, and a setter there keeps a
/// constructor's `this.f = v` from defining the birth field `f`. So when the
/// program can run interpreted code, every elided field access whose proof
/// rests on the facts tests this word (one load) and takes its complete
/// generic operation once it is set (`perry-codegen` `wpfacts_access.rs`).
/// Writes by compiled code are the facts' own (E5/E6/E20/E23) and do not set
/// it.
///
/// Lives here (always compiled), not in `crate::dyn_eval`: the funnels and
/// generated code reference it in every build.
///
/// Set from every prototype mutation funnel: [`note_proto_write`] is called
/// by the sealed-intrinsic write funnel (`builtin_proto::note_family_write`:
/// slot and spill stores, key adds, deletes, descriptor and attribute
/// changes, relinks of a family prototype) and by the marked-prototype
/// funnels (`proto_validity::note_object_shape_stamped` /
/// `note_object_own_keys_changed`, `builtin_proto::note_prototype_value_store`).
#[no_mangle]
pub static PERRY_INTERP_PROTO_WRITTEN: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

/// A prototype is being written: set [`PERRY_INTERP_PROTO_WRITTEN`] when
/// interpreted code is on this thread's stack.
#[inline]
pub(crate) fn note_proto_write() {
    use std::sync::atomic::Ordering;
    if PERRY_INTERP_PROTO_WRITTEN.load(Ordering::Relaxed) != 0 {
        return;
    }
    if INTERP_ACTIVE.with(|c| c.get()) != 0 {
        PERRY_INTERP_PROTO_WRITTEN.store(1, Ordering::Relaxed);
    }
}

std::thread_local! {
    /// Entries into interpreted code now on this thread's stack
    /// (`dyn_eval::interp_entry`): non-zero while interpreted code, or
    /// anything it called, runs.
    static INTERP_ACTIVE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// `dyn_eval::interp_entry` entered interpreted code.
#[cfg_attr(not(feature = "dyn-eval"), allow(dead_code))]
pub(crate) fn interp_active_enter() {
    INTERP_ACTIVE.with(|c| c.set(c.get() + 1));
}

/// `dyn_eval::interp_entry` left it (returned or threw).
#[cfg_attr(not(feature = "dyn-eval"), allow(dead_code))]
pub(crate) fn interp_active_leave() {
    INTERP_ACTIVE.with(|c| c.set(c.get().saturating_sub(1)));
}
