//! The script evaluator's (`crate::dyn_eval`) touch points in always-live code.
//!
//! The `Function` constructor, dynamic `import()` of a JavaScript `data:` URL,
//! the GC (a root scanner, the move hook, the dead-owner prune) and the
//! exception savepoints all reach the interpreter. Those callers are live in
//! every program, so they must not name `crate::dyn_eval` directly, or the
//! prebuilt full-feature runtime keeps the interpreter and the JS parser behind
//! it in every binary. They call these forwarders, which reach it only through
//! slots the `dyn-eval` install fills (see `crate::feature_hooks`).
//!
//! The install runs from `js_gc_init`, before any user code, so the
//! interpreter never runs with a slot empty. An empty slot answers what a build
//! without `dyn-eval` answers: no function, no scan, no move bookkeeping, an
//! idle savepoint.

use crate::feature_hooks::Hook;

static FUNCTION_FROM_STRINGS: Hook<fn(&[String]) -> f64> = Hook::empty();
static SCAN_ROOTS: Hook<fn(&mut crate::gc::RuntimeRootVisitor<'_>)> = Hook::empty();
static OWNER_MOVED: Hook<fn(usize, usize)> = Hook::empty();
static PRUNE_DEAD_OWNERS: Hook<fn(&dyn Fn(usize) -> bool)> = Hook::empty();
static PRUNE_DEAD_OWNERS_YOUNG: Hook<fn(&dyn Fn(usize) -> bool)> = Hook::empty();
static INTERP_SAVEPOINT: Hook<fn() -> u64> = Hook::empty();
static INTERP_RESTORE: Hook<fn(u64)> = Hook::empty();
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

pub(crate) fn function_owner_moved(old: usize, new: usize) {
    if let Some(moved) = OWNER_MOVED.get() {
        moved(old, new);
    }
}

pub(crate) fn prune_dead_function_owners(is_dead: &dyn Fn(usize) -> bool) {
    if let Some(prune) = PRUNE_DEAD_OWNERS.get() {
        prune(is_dead);
    }
}

pub(crate) fn prune_dead_function_owners_young(is_dead: &dyn Fn(usize) -> bool) {
    if let Some(prune) = PRUNE_DEAD_OWNERS_YOUNG.get() {
        prune(is_dead);
    }
}

/// The interpreter's `try` savepoint; `0` (the catch table's idle value)
/// without the evaluator.
///
/// `js_try_push` runs this on every `try`, and the GC call-effects tables
/// (`crates/perry-codegen/src/gc_effects/`) keep `js_try_push` Leaf only
/// because `scripts/gc_call_effects/seeds.txt` delegates this function's
/// indirect call to its one possible target, `crate::dyn_eval::interp_savepoint`
/// (the only value [`install`] stores in the slot). `#[inline(never)]` keeps
/// the indirect call in this named symbol, where that rule can match it; a
/// closure (`map_or(0, |f| f())`) would move it into an unnamed one.
#[inline(never)]
pub(crate) fn interp_savepoint() -> u64 {
    match INTERP_SAVEPOINT.get() {
        Some(savepoint) => savepoint(),
        None => 0,
    }
}

/// Restore the interpreter to a `try` savepoint. Delegated in seeds.txt to
/// `crate::dyn_eval::interp_restore`, like [`interp_savepoint`].
#[inline(never)]
pub(crate) fn interp_restore(savepoint: u64) {
    if let Some(restore) = INTERP_RESTORE.get() {
        restore(savepoint);
    }
}

/// Dynamic `import()` of a JavaScript `data:` URL; `None` without the
/// evaluator, which leaves the loader's "cannot find module" path.
pub(crate) fn dynamic_import_data_url(specifier: &str) -> Option<f64> {
    DATA_URL_IMPORT.get().and_then(|f| f(specifier))
}

/// The `dyn-eval` install: the only writer of these slots. The GC
/// call-effects rules for [`interp_savepoint`] / [`interp_restore`] name the
/// targets stored here; change both together.
#[cfg(feature = "dyn-eval")]
pub(crate) fn install() {
    FUNCTION_FROM_STRINGS.set(crate::dyn_eval::dyn_function_from_strings);
    SCAN_ROOTS.set(crate::dyn_eval::scan_dyn_eval_roots_mut);
    OWNER_MOVED.set(crate::dyn_eval::function_owner_moved);
    PRUNE_DEAD_OWNERS.set(crate::dyn_eval::prune_dead_function_owners);
    PRUNE_DEAD_OWNERS_YOUNG.set(crate::dyn_eval::prune_dead_function_owners_young);
    INTERP_SAVEPOINT.set(crate::dyn_eval::interp_savepoint);
    INTERP_RESTORE.set(crate::dyn_eval::interp_restore);
    DATA_URL_IMPORT.set(crate::module_require::dynamic_import_javascript_data_url);
}
