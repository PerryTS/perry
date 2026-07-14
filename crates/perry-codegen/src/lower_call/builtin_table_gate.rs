//! Import-source gate for the `perry/system` | `perry/updater` | `perry/background`
//! dispatch tables (issue #6087).

use crate::expr::FnCtx;

/// Issue #6087 — may the `perry/system` | `perry/updater` | `perry/background`
/// dispatch table claim a call to `name`?
///
/// Those three tables are keyed by bare TypeScript name (`takeScreenshot`,
/// `openURL`, `getLocale`, `preferencesGet`, `hapticPlay`, `schedule`, …) and
/// used to be consulted on the name alone. But a function the user *imported
/// from their own module* lowers to exactly the same `Expr::ExternFuncRef {
/// name }` as a `perry/system` import does, so any user function whose name
/// collided with one of those ~60 rows was hijacked into the native table:
///
/// * arity differs from the native row → `lower_perry_ui_table_call` dropped
///   the call on the floor (silent miscompile — the reported symptom);
/// * arity happens to match → the program links against an undefined
///   `perry_system_*` symbol even though it imports nothing native.
///
/// `imported_class_sources` maps every named/default import binding in *this*
/// module to the specifier it was imported from, which answers the question
/// exactly: a binding that came from anywhere other than `module` can never be
/// the native builtin, so the table must not claim it. The cross-module inliner
/// keeps this sound — when it moves a body containing an `ExternFuncRef` into
/// another module it also adds the matching `Import` to the destination's
/// `hir.imports` (see `inline::cross_module`), which is the same table this map
/// is built from.
///
/// A name with *no* import binding in this module (ambient `declare`s,
/// synthesized extern refs) has no import source to contradict the table, so it
/// keeps the historical name-only behaviour. Note this also means a `perry/*`
/// import that never reaches `hir.imports` still dispatches as before — the
/// gate can only ever *reject* a name that demonstrably came from elsewhere.
pub(super) fn callee_is_from_perry_module(ctx: &FnCtx<'_>, name: &str, module: &str) -> bool {
    match ctx.imported_class_sources.get(name) {
        Some(source) => source == module,
        None => true,
    }
}
