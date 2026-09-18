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

/// The ABI of a class's standalone `<class>_constructor` symbol: how many slots
/// it takes and which of the trailing ones are arrays a caller has to PACK — a
/// user `...rest` and/or the HIR-synthesized `arguments` slot (#10484).
///
/// A class with no own constructor emits the `super(...args)` forwarder, which
/// passes every slot on to the ancestor's symbol unchanged, so it carries the
/// ancestor's ABI verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CtorAbi {
    pub param_count: usize,
    pub has_rest: bool,
    pub has_synthetic_arguments: bool,
}

impl CtorAbi {
    /// Positional-only, `param_count` slots.
    pub(crate) fn positional(param_count: usize) -> Self {
        CtorAbi {
            param_count,
            ..CtorAbi::default()
        }
    }

    /// Read a constructor's own parameter list. Synthesized `__perry_cap_*`
    /// capture params trail the arrays and are bound from the class's capture
    /// snapshot rather than from the call, so a capturing constructor reports
    /// no packable slot and keeps positional marshaling.
    pub(crate) fn from_params(params: &[perry_hir::Param]) -> Self {
        if params.iter().any(|p| p.name.starts_with("__perry_cap_")) {
            return CtorAbi::positional(params.len());
        }
        CtorAbi {
            param_count: params.len(),
            has_rest: params
                .iter()
                .any(|p| p.is_rest && p.arguments_object.is_none()),
            has_synthetic_arguments: params.iter().any(|p| p.arguments_object.is_some()),
        }
    }
}

/// The standalone-constructor ABI of `class` when it can be decided from the
/// class definition alone, without the defining module's class table or
/// imports. The source-graph constructor-contract resolver uses this, so it
/// MUST agree with [`synthesized_ctor_param_count`] for every case it answers:
/// an own constructor, a native parent, no heritage, and a heritage that is only
/// a runtime value (`extends_expr` with no resolvable `extends_name`), which
/// always synthesizes the fixed positional forwarding band. `None` means the ABI
/// depends on the defining module's ancestor walk (#10258).
pub fn context_free_ctor_abi(class: &perry_hir::Class) -> Option<CtorAbi> {
    if let Some(c) = class.constructor.as_ref() {
        return Some(CtorAbi::from_params(&c.params));
    }
    if class.native_extends.is_some() {
        return Some(CtorAbi::positional(0));
    }
    match (&class.extends_name, &class.extends_expr) {
        (None, None) => Some(CtorAbi::positional(0)),
        (None, Some(_)) => Some(CtorAbi::positional(UNRESOLVED_PARENT_FWD_ARITY)),
        _ => None,
    }
}

/// [`context_free_ctor_abi`]'s arity half.
pub fn context_free_ctor_param_count(class: &perry_hir::Class) -> Option<usize> {
    context_free_ctor_abi(class).map(|abi| abi.param_count)
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

/// The parameter list whose trailing-array layout (user rest, synthesized
/// `arguments`) the emitted standalone `<class>_constructor` symbol follows.
///
/// An own constructor is its own layout. A class with NO own constructor emits
/// the `super(...args)` forwarder, whose `__forward_arg<i>` params adopt the
/// nearest ctor-bearing LOCAL ancestor's params positionally and pass every slot
/// to that ancestor's symbol unchanged, so a caller has to fill them exactly as
/// it would fill the ancestor's (#10484: the ancestor's `arguments` slot must
/// receive the whole argument list, not one positional argument).
///
/// `None` wherever the forwarder does not make that static positional call: a
/// dynamic heritage edge (`extends_expr`, forwarded through the runtime
/// dynamic-parent dispatcher as plain arguments), a native parent, an imported
/// ancestor, or an arity that does not match `emitted_param_count`. Also `None`
/// for an ancestor with `__perry_cap_*` params: the forwarder registers no
/// capture slots, so its callers bind every slot positionally as before.
pub(super) fn constructor_layout_params<'a>(
    class: &'a perry_hir::Class,
    class_table: &HashMap<String, &'a perry_hir::Class>,
    emitted_param_count: u32,
) -> Option<&'a [perry_hir::Param]> {
    if let Some(ctor) = class.constructor.as_ref() {
        return Some(&ctor.params);
    }
    if class.native_extends.is_some() || class.extends_expr.is_some() {
        return None;
    }
    let mut parent = class.extends_name.as_deref();
    let mut depth = 0usize;
    while let Some(name) = parent {
        let ancestor = *class_table.get(name)?;
        // Imported stubs carry id 0 and no constructor body in this module.
        if ancestor.id == 0 || ancestor.native_extends.is_some() || depth > 64 {
            return None;
        }
        if let Some(ctor) = ancestor.constructor.as_ref() {
            let positional = ctor.params.len() == emitted_param_count as usize
                && !ctor
                    .params
                    .iter()
                    .any(|p| p.name.starts_with("__perry_cap_"));
            return positional.then_some(ctor.params.as_slice());
        }
        if ancestor.extends_expr.is_some() {
            return None;
        }
        parent = ancestor.extends_name.as_deref();
        depth += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::types::Type;
    use perry_hir::{ArgumentsObjectMeta, Class, Function, Param};

    fn param(name: &str, is_rest: bool, arguments_object: bool) -> Param {
        Param {
            id: 0,
            name: name.to_string(),
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest,
            arguments_object: arguments_object.then(|| ArgumentsObjectMeta {
                strict: true,
                simple_parameters: false,
                mapped_parameter_ids: Vec::new(),
                restricted_callee: true,
            }),
        }
    }

    fn class(name: &str, extends: Option<&str>, ctor_params: Option<Vec<Param>>) -> Class {
        Class {
            id: 1,
            name: name.to_string(),
            type_params: Vec::new(),
            extends: None,
            extends_name: extends.map(|e| e.to_string()),
            native_extends: None,
            extends_expr: None,
            heritage_lexically_shadowed: false,
            fields: Vec::new(),
            constructor: ctor_params.map(|params| Function {
                id: 7,
                name: format!("{}_constructor", name),
                type_params: Vec::new(),
                params,
                return_type: Type::Void,
                body: Vec::new(),
                is_async: false,
                is_generator: false,
                is_strict: true,
                is_exported: false,
                captures: Vec::new(),
                decorators: Vec::new(),
                was_plain_async: false,
                was_unrolled: false,
            }),
            methods: Vec::new(),
            getters: Vec::new(),
            setters: Vec::new(),
            static_accessor_names: Vec::new(),
            static_accessor_fn_ids: Vec::new(),
            static_fields: Vec::new(),
            static_methods: Vec::new(),
            computed_members: Vec::new(),
            decorators: Vec::new(),
            is_exported: false,
            is_nested: false,
            alloc_width_hint: 0,
            specialized_from: None,
            aliases: Vec::new(),
        }
    }

    fn arguments_ctor_params() -> Vec<Param> {
        vec![
            param("p", false, false),
            param("q", false, false),
            param("arguments", true, true),
        ]
    }

    #[test]
    fn reads_the_trailing_arrays_of_an_own_constructor() {
        let abi = CtorAbi::from_params(&arguments_ctor_params());
        assert_eq!(
            abi,
            CtorAbi {
                param_count: 3,
                has_rest: false,
                has_synthetic_arguments: true,
            }
        );
        let with_user_rest = vec![
            param("first", false, false),
            param("rest", true, false),
            param("arguments", false, true),
        ];
        assert_eq!(
            CtorAbi::from_params(&with_user_rest),
            CtorAbi {
                param_count: 3,
                has_rest: true,
                has_synthetic_arguments: true,
            }
        );
    }

    #[test]
    fn a_capturing_constructor_reports_no_packable_slot() {
        // The capture params trail the arrays, so the packed slots are not the
        // last ones and every caller binds positionally instead.
        let mut params = arguments_ctor_params();
        params.push(param("__perry_cap_4", false, false));
        assert_eq!(CtorAbi::from_params(&params), CtorAbi::positional(4));
    }

    #[test]
    fn a_forwarder_adopts_its_local_ancestors_layout() {
        let base = class("Base", None, Some(arguments_ctor_params()));
        let middle = class("Middle", Some("Base"), None);
        let leaf = class("Leaf", Some("Middle"), None);
        let table: HashMap<String, &Class> = [
            ("Base".to_string(), &base),
            ("Middle".to_string(), &middle),
            ("Leaf".to_string(), &leaf),
        ]
        .into_iter()
        .collect();

        let layout = constructor_layout_params(&leaf, &table, 3).expect("ancestor layout");
        assert!(layout.iter().any(|p| p.arguments_object.is_some()));
        assert_eq!(layout.len(), 3);

        // An arity the emitted forwarder does not have must not claim a layout:
        // the caller would pack an array into a slot nobody forwards.
        assert!(constructor_layout_params(&leaf, &table, 2).is_none());
    }

    #[test]
    fn a_dynamic_or_unknown_heritage_forwarder_has_no_layout() {
        let base = class("Base", None, Some(arguments_ctor_params()));
        let mut dynamic = class("Dynamic", Some("Base"), None);
        dynamic.extends_expr = Some(Box::new(perry_hir::Expr::LocalGet(3)));
        let orphan = class("Orphan", Some("Missing"), None);
        let table: HashMap<String, &Class> = [
            ("Base".to_string(), &base),
            ("Dynamic".to_string(), &dynamic),
            ("Orphan".to_string(), &orphan),
        ]
        .into_iter()
        .collect();

        assert!(constructor_layout_params(&dynamic, &table, 3).is_none());
        assert!(constructor_layout_params(&orphan, &table, 3).is_none());
    }
}
