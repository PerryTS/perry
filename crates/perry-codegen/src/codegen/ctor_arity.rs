//! Synthesized standalone-constructor arity for classes that inherit the JS
//! spec default ctor `constructor(...args) { super(...args) }`.
//!
//! Split out of `codegen/artifacts.rs` for the 2000-line file cap (#8204 took
//! it to 2005). Pure code move — no logic change.

use std::collections::{BTreeMap, HashMap};

/// Positional forwarding band for a synthesized default ctor whose parent arity
/// cannot be resolved while compiling the defining module (see the tail of
/// [`synthesized_ctor_param_count`]).
pub const UNRESOLVED_PARENT_FWD_ARITY: usize = 8;

/// The standalone-constructor arity of `class` when it can be decided from the
/// class definition alone, without the defining module's class table or
/// imports. The source-graph constructor-contract resolver uses this, so it
/// MUST agree with [`synthesized_ctor_param_count`] for every case it answers:
/// an own constructor, a native parent, no heritage, and a heritage that is only
/// a runtime value (`extends_expr` with no resolvable `extends_name`), which
/// always synthesizes the fixed forwarding band. `None` means the arity depends
/// on the defining module's ancestor walk (#10258).
pub fn context_free_ctor_param_count(class: &perry_hir::Class) -> Option<usize> {
    if let Some(c) = class.constructor.as_ref() {
        return Some(c.params.len());
    }
    if class.native_extends.is_some() {
        return Some(0);
    }
    match (&class.extends_name, &class.extends_expr) {
        (None, None) => Some(0),
        (None, Some(_)) => Some(UNRESOLVED_PARENT_FWD_ARITY),
        _ => None,
    }
}

/// The standalone-constructor arity Perry emits for `class`, accounting for the
/// JS spec default ctor `constructor(...args) { super(...args) }` that a class
/// with NO own constructor but WITH heritage inherits. Walks the ancestor chain
/// (local `class_table` + cross-module `imported_classes` ctor-param counts +
/// imported stubs) for the nearest ctor-bearing parent's user arity, mirroring
/// the `found_params` walk in the per-class ctor emission below.
///
/// This MUST agree with the synthesized standalone ctor's actual LLVM signature,
/// otherwise the runtime registers a different `total_params` than the function
/// declares and `replay_registered_class_constructor` forwards the wrong number
/// of args (Next.js wall 51: `class AppRouteRouteMatcher extends
/// _mod.RouteMatcher {}` emitted a 1-param forwarding ctor but registered it as
/// 0 params, so `new mod.AppRouteRouteMatcher(def)` dropped `def` before
/// `RouteMatcher(definition)` ran and every matcher's `this.definition` was a
/// garbage number).
pub(super) fn synthesized_ctor_param_count(
    class: &perry_hir::Class,
    class_table: &HashMap<String, &perry_hir::Class>,
    imported_class_stubs: &[perry_hir::Class],
    imported_classes: &[super::opts::ImportedClass],
    resolved_arities: &BTreeMap<String, usize>,
) -> usize {
    // Source-graph builds resolve once, in the defining module's scope, and
    // copy this exact contract to every import before hashing either object.
    // Standalone/auto-optimize callers keep the existing ancestor walk below.
    if let Some(count) = resolved_arities.get(&class.name) {
        return *count;
    }

    if let Some(c) = class.constructor.as_ref() {
        return c.params.len();
    }
    // A native parent (`extends Error` / `extends events.EventEmitter`) has its
    // own native construction path that consumes the construction args directly;
    // don't synthesize a forwarding ctor for it.
    if class.native_extends.is_some() {
        return 0;
    }
    // No heritage at all → nothing to forward.
    if class.extends_name.is_none() && class.extends_expr.is_none() {
        return 0;
    }
    // Walk ancestors for the nearest ctor-bearing parent's user arity.
    let mut cur = class.extends_name.clone();
    while let Some(pname) = cur {
        let imported_ctor_params = imported_classes
            .iter()
            .find(|i| i.effective_name() == pname)
            .map(|ic| ic.constructor_param_count)
            .unwrap_or(0);
        if let Some(pclass) = class_table.get(pname.as_str()) {
            if let Some(pctor) = &pclass.constructor {
                return pctor.params.len();
            }
            if imported_ctor_params > 0 {
                return imported_ctor_params;
            }
            cur = pclass.extends_name.clone();
        } else if let Some(stub) = imported_class_stubs.iter().find(|c| c.name == pname) {
            if imported_ctor_params > 0 {
                return imported_ctor_params;
            }
            cur = stub.extends_name.clone();
        } else {
            break;
        }
    }
    // The parent's exact ctor arity is genuinely unavailable here in some build
    // modes: the auto-optimize / standalone path compiles each nested
    // `node_modules` module with an EMPTY `imported_classes` list and resolves
    // the cross-module parent purely as a runtime DYNAMIC parent
    // (`extends_expr` + `js_register_class_parent_dynamic`), so it is absent
    // from `class_table` and `imported_class_stubs` here. Without a forwarding
    // signature the synthesized `super()` dropped every construction arg
    // (Next.js wall 51: `class PagesRouteMatcher extends _mod.RouteMatcher {}`
    // → `RouteMatcher(definition)` saw garbage → every matcher's
    // `this.definition` was undefined). Forward a generous fixed band of
    // positional params: the `new` site pads missing slots with `undefined`,
    // and a parent ctor reading fewer params ignores the trailing `undefined`s,
    // so over-declaring is correct for any (non-native) parent up to this band.
    UNRESOLVED_PARENT_FWD_ARITY
}
