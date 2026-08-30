//! Synthesized standalone-constructor arity for classes that inherit the JS
//! spec default ctor `constructor(...args) { super(...args) }`.
//!
//! Split out of `codegen/artifacts.rs` for the 2000-line file cap (#8204 took
//! it to 2005). Pure code move — no logic change.

use std::collections::HashMap;

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
) -> usize {
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
    const UNRESOLVED_PARENT_FWD_ARITY: usize = 8;
    UNRESOLVED_PARENT_FWD_ARITY
}
