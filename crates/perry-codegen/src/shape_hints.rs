//! Compile-time shape hints for module-global receivers (design step 4).
//!
//! # The fact
//!
//! `const O = { a: 1, b: 2 }` at module scope lowers to a module-global local
//! whose HIR initializer is `New { class_name: "__AnonShape_<hash of keys>" }`.
//! The shape is therefore known at compile time — but `receiver_class_name`
//! does not answer for it: the local's HIR type is `Object(ObjectType)`, not
//! `Named`, and the Ptr<Shape> proof deliberately excludes module globals
//! (`collectors/ptr_shape.rs` seeds candidates without them, because a module
//! global can be reached from anywhere and no single-owner proof is possible).
//!
//! The single-owner proof is not what a SHAPE guard needs. A guard that
//! compares the receiver's ShapeId word against a constant is correct for ANY
//! receiver; being wrong about which shape shows up costs a missed guard, not
//! a wrong value. So this map is deliberately a HINT and never a proof, and
//! nothing downstream may treat it as one.
//!
//! # Why a scoped thread-local and not a `FnCtx` field
//!
//! `FnCtx` is built at six sites across five files that other lanes are
//! actively editing, and threading one more module-level map through all of
//! them would collide with them for no behavioural gain. `unit_cache`'s
//! `with_module_cache` establishes the pattern: one module's codegen runs to
//! completion on one thread (perry-codegen uses no rayon internally — the same
//! argument `ext_registry`'s `MODULE_CAPTURE` relies on), so a scoped
//! thread-local is exactly as module-scoped as a field would be.

use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// Module-global `LocalId` -> the `__AnonShape_*` class its initializer
    /// constructs. Empty outside an installed scope, which makes every
    /// consult answer `None` and every site fall back to today's tower.
    static HINTS: RefCell<HashMap<u32, String>> = RefCell::new(HashMap::new());
}

/// Clears the installed hints on drop, including on unwind.
pub(crate) struct ModuleHintScope;

impl Drop for ModuleHintScope {
    fn drop(&mut self) {
        HINTS.with(|h| h.borrow_mut().clear());
    }
}

/// Install this module's hints for the duration of the returned guard.
pub(crate) fn install(hints: HashMap<u32, String>) -> ModuleHintScope {
    HINTS.with(|h| *h.borrow_mut() = hints);
    ModuleHintScope
}

/// The `__AnonShape_*` class a module-global local is initialized with.
pub(crate) fn anon_shape_class_for_global(local_id: u32) -> Option<String> {
    HINTS.with(|h| h.borrow().get(&local_id).cloned())
}

/// Build the map from a module's top-level statements.
///
/// Only object literals (`__AnonShape_*`) are recorded, and only when the
/// initializer is directly a `New`. Two restrictions, both deliberate:
///
/// * an `__AnonShape_*` class has no private and no computed fields, so
///   `class_field_global_index`'s key count and the packed-keys builder's key
///   count agree — they filter differently (`key_expr.is_none()` versus
///   `key_expr.is_none() && !f.is_private`) and the constant slot this hint
///   licenses is only sound where they cannot disagree;
/// * a declared class would need its inheritance chain resolved here, which
///   `class_init_chains` already does properly at the use site. Extending the
///   hint to those is follow-on work, not a shortcut to take now.
pub(crate) fn collect_module_global_shapes(hir: &perry_hir::Module) -> HashMap<u32, String> {
    use perry_hir::{Expr, Stmt};
    let mut out: HashMap<u32, String> = HashMap::new();
    for stmt in &hir.init {
        let Stmt::Let { id, init, .. } = stmt else {
            continue;
        };
        let Some(Expr::New { class_name, .. }) = init.as_ref() else {
            continue;
        };
        if class_name.starts_with("__AnonShape_") {
            out.insert(*id, class_name.clone());
        }
    }

    // Destructuring binds the receiver to a temporary first, so without this
    // pass `const { a, b, c, d, e } = O` — five reads of one receiver in one
    // statement, the case the sharing is FOR — is not hinted at all. The HIR
    // is explicit about it:
    //
    //   Let { id: 10, name: "__destruct_10",
    //         init: NativeMethodCall { method: "requireObjectCoercible",
    //                                  args: [LocalGet(0)] } }
    //   Let { id: 11, name: "a", init: PropertyGet { object: LocalGet(10), … } }
    //
    // `requireObjectCoercible` returns its argument or throws, so the
    // temporary IS the global. Keyed by LocalId with no scope qualifier
    // because HIR LocalIds are globally unique within a module — the property
    // `collect_module_local_types` already relies on for the same reason.
    let mut aliases: HashMap<u32, u32> = HashMap::new();
    collect_coercible_aliases(&hir.init, &mut aliases);
    for function in &hir.functions {
        collect_coercible_aliases(&function.body, &mut aliases);
    }
    for class in &hir.classes {
        for method in class
            .methods
            .iter()
            .chain(class.static_methods.iter())
            .chain(class.constructor.iter())
        {
            collect_coercible_aliases(&method.body, &mut aliases);
        }
    }
    for (alias, target) in aliases {
        if let Some(class_name) = out.get(&target).cloned() {
            out.insert(alias, class_name);
        }
    }
    out
}

/// Record `let t = requireObjectCoercible(LocalGet(g))` as `t -> g`.
///
/// Mirrors `boxed_vars::collect_let_types_in_stmts`' descent. A container this
/// does not descend into costs a missed hint and nothing else, which is why it
/// is written as a best-effort walk rather than an exhaustive one.
fn collect_coercible_aliases(stmts: &[perry_hir::Stmt], out: &mut HashMap<u32, u32>) {
    use perry_hir::{Expr, Stmt};
    for stmt in stmts {
        match stmt {
            Stmt::Let {
                id,
                init: Some(Expr::NativeMethodCall { method, args, .. }),
                ..
            } if method == "requireObjectCoercible" => {
                if let Some(Expr::LocalGet(target)) = args.first() {
                    out.insert(*id, *target);
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_coercible_aliases(then_branch, out);
                if let Some(eb) = else_branch {
                    collect_coercible_aliases(eb, out);
                }
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    collect_coercible_aliases(std::slice::from_ref(init_stmt.as_ref()), out);
                }
                collect_coercible_aliases(body, out);
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_coercible_aliases(body, out);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_coercible_aliases(body, out);
                if let Some(c) = catch {
                    collect_coercible_aliases(&c.body, out);
                }
                if let Some(f) = finally {
                    collect_coercible_aliases(f, out);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_coercible_aliases(&case.body, out);
                }
            }
            Stmt::Labeled { body, .. } => {
                collect_coercible_aliases(std::slice::from_ref(body.as_ref()), out);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::{Expr, Module, Stmt};

    fn module_with(init: Vec<Stmt>) -> Module {
        let mut m = Module::new("hint.ts");
        m.init = init;
        m
    }

    fn anon_new(class_name: &str) -> Expr {
        Expr::New {
            class_name: class_name.to_string(),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
            capture_arg_count: 0,
        }
    }

    fn let_stmt(id: u32, name: &str, init: Option<Expr>) -> Stmt {
        Stmt::Let {
            id,
            name: name.to_string(),
            ty: perry_hir::types::Type::Any,
            mutable: false,
            init,
        }
    }

    /// The population the guard is emitted for: a module-level `const` bound
    /// to an object literal. Fails without the collector.
    #[test]
    fn a_module_global_object_literal_is_hinted() {
        let hir = module_with(vec![let_stmt(1, "O", Some(anon_new("__AnonShape_abc123")))]);
        let hints = collect_module_global_shapes(&hir);
        assert_eq!(hints.get(&1).map(String::as_str), Some("__AnonShape_abc123"));
    }

    /// Everything else must be absent, because a hint the compiler cannot
    /// justify costs a failing compare on every read of that site. A DECLARED
    /// class is excluded deliberately: `class_field_global_index` and the
    /// packed-keys builder filter private fields differently, and the constant
    /// slot is only sound where they cannot disagree.
    #[test]
    fn a_declared_class_and_a_non_new_initializer_are_not_hinted() {
        let hir = module_with(vec![
            let_stmt(1, "c", Some(anon_new("Point"))),
            let_stmt(2, "n", Some(Expr::Number(1.0))),
            let_stmt(3, "u", None),
        ]);
        let hints = collect_module_global_shapes(&hir);
        assert!(hints.is_empty(), "unexpected hints: {hints:?}");
    }

    /// The scope must not leak into the next module compiled on this thread —
    /// a stale hint would name another module's absolute symbol, and the link
    /// would fail on an undefined one.
    #[test]
    fn hints_do_not_outlive_their_scope() {
        {
            let _scope = install(collect_module_global_shapes(&module_with(vec![let_stmt(
                7,
                "O",
                Some(anon_new("__AnonShape_scoped")),
            )])));
            assert_eq!(
                anon_shape_class_for_global(7).as_deref(),
                Some("__AnonShape_scoped")
            );
        }
        assert_eq!(anon_shape_class_for_global(7), None);
    }
}
