// This module is part of the perry-codegen-arkts crate. It was
// mechanically split out of the former monolithic lib.rs (issue
// #1100). Pure code move — no logic changes.
#![allow(clippy::too_many_arguments)]
use crate::*;

pub(crate) fn collect_state_bindings(init: &[Stmt]) -> HashMap<LocalId, StateBinding> {
    let mut map = HashMap::new();
    let mut counter: usize = 0;
    for stmt in init {
        if let Stmt::Let {
            id,
            init: Some(call_expr),
            ..
        } = stmt
        {
            let initial = match call_expr {
                // Match either `Expr::NativeMethodCall { module: "perry/ui", method: "state", args: [v] }`
                // OR `Expr::Call { callee: Ident("state"), args: [v] }` (whichever
                // shape the perry-hir lowerer produces for the import).
                Expr::NativeMethodCall {
                    module,
                    method,
                    object: None,
                    args,
                    ..
                } if module == "perry/ui" && method == "state" && args.len() == 1 => {
                    Some(args[0].clone())
                }
                _ => None,
            };
            if let Some(initial_expr) = initial {
                let synth_id = format!("__state_{}", counter);
                counter += 1;
                let initial_str = match &initial_expr {
                    Expr::String(s) => s.clone(),
                    Expr::Number(n) => fmt_num(*n),
                    Expr::Integer(n) => format!("{}", n),
                    Expr::Bool(b) => format!("{}", b),
                    _ => "".to_string(),
                };
                map.insert(
                    *id,
                    StateBinding {
                        synth_id,
                        initial_str,
                    },
                );
            }
        }
    }
    map
}

/// Phase 2 v3.5 — pre-walk for `widgetSetHidden(LocalGet(target), _)` calls
/// across the ENTIRE module (init + every function body + every closure body
/// recursively). Targets that get touched in any non-init scope earn a
/// `VisibilityBinding` with a synth-id; the harvest then emits a bound
/// `.visibility(this.hidden_<id> ? Hidden : Visible)` modifier on the
/// widget instead of the static `.visibility(Visibility.X)` it would
/// otherwise produce, AND the closure-body call sites are HIR-rewritten to
/// route through the NAPI drain queue.
///
/// Initial value: walks `module.init` only, picking the LAST literal
/// `widgetSetHidden(target, V)` it finds. Latest-wins matches Mango's
/// pattern where the file might call `widgetSetHidden(formContainer, 0)`
/// then `widgetSetHidden(formContainer, 1)` at module-init top-level (the
/// second is the actual initial state). Non-literal init values fall
/// through to `false` (visible) — same default as widgets in general.
pub(crate) fn collect_visibility_bindings(module: &Module) -> HashMap<LocalId, VisibilityBinding> {
    let mut map: HashMap<LocalId, VisibilityBinding> = HashMap::new();
    let mut counter: usize = 0;

    // Pass 1 — discover all targets reached from runtime call paths only.
    // Module-init TOP-LEVEL `widgetSetHidden(target, V)` calls stay static
    // (the v0.5.480 collect_mutations path emits `.visibility(Visibility.X)`
    // directly). A target earns a binding only when there's a call site
    // that fires AT RUNTIME — i.e. inside a function body that's invoked
    // post-mount, or inside a closure (anywhere). Module-init init-time
    // calls are out of scope by design.
    let mut targets: std::collections::BTreeSet<LocalId> = std::collections::BTreeSet::new();
    walk_init_for_closure_targets(&module.init, &mut targets);
    for f in &module.functions {
        walk_for_set_hidden_targets_in_stmts(&f.body, &mut targets);
    }

    // Stable synth-id assignment by sorted LocalId (BTreeSet iteration is
    // ordered) so re-running the harvest produces the same .ets bytes.
    for target_id in &targets {
        let synth_id = format!("vis_{}", counter);
        counter += 1;
        map.insert(
            *target_id,
            VisibilityBinding {
                synth_id,
                initial_hidden: false, // overwritten below if a literal init call is found
            },
        );
    }

    // Pass 2 — walk module.init only for initial value detection.
    // Only top-level Stmt::Expr is considered; nested if/loop init values
    // intentionally skip (the runtime branch only fires after main()
    // returns control to ArkUI in the harvest model anyway).
    for stmt in &module.init {
        if let Stmt::Expr(e) = stmt {
            if let Some((target_id, hide)) = extract_widget_set_hidden_literal(e) {
                if let Some(binding) = map.get_mut(&target_id) {
                    binding.initial_hidden = hide;
                }
            }
        }
    }

    map
}

/// Snapshot read-only top-level `let x = <expr>;` so widget walks can
/// follow `Expr::LocalGet(x)` back to the init expression. We index by
/// LocalId rather than name because perry-hir's identifier resolution
/// runs by id — names are debug aids only.
///
/// Phase 2 v1.5 only follows TOP-level inits; nested let-bindings inside
/// blocks would need a wider analysis pass (the code path is only invoked
/// via `App({body: x})` which itself is top-level, so the binding it
/// references is also top-level — works for the common case).
pub(crate) fn collect_const_bindings(init: &[Stmt]) -> HashMap<LocalId, Expr> {
    let mut map = HashMap::new();
    walk_collect_const_bindings(init, &mut map);
    map
}

/// Recursive helper for `collect_const_bindings` — walks `Stmt::If` /
/// `Stmt::Block` bodies so const bindings created in conditional branches
/// are visible to the harvest. Mango's pattern:
///
/// ```ts
/// if (mobile) {
///     const connInfoBtn = Button(...);
///     widgetAddChild(connBody, HStack([Spacer(), connInfoBtn, Spacer()]));
/// }
/// ```
///
/// — the `widgetAddChild` mutation gets recorded by `collect_mutations`
/// with its enclosing condition. The inner Button construction needs
/// `connInfoBtn` to be in `bindings` when the harvest emits the inner
/// HStack children — otherwise it falls through to `[unrecognized body]`.
///
/// Limitation: if two if-branches both `const foo = ...` with different
/// RHS, the last branch's binding wins. Mango doesn't hit this (each
/// branch defines unique names) and the alternative — full scoped
/// resolution — is meaningfully more complex. Acceptable trade-off for
/// the procedural-construction use case.
pub(crate) fn walk_collect_const_bindings(stmts: &[Stmt], map: &mut HashMap<LocalId, Expr>) {
    for stmt in stmts {
        match stmt {
            Stmt::Let {
                id,
                init: Some(expr),
                mutable: false,
                ..
            } => {
                map.insert(*id, expr.clone());
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                walk_collect_const_bindings(then_branch, map);
                if let Some(eb) = else_branch {
                    walk_collect_const_bindings(eb, map);
                }
            }
            _ => {}
        }
    }
}

/// Issue #410 — discover `declare const __platform__: number;` style
/// compile-time constants. The HIR shape is `Stmt::Let { name, init: None }`
/// (matches `crates/perry-codegen/src/codegen.rs::compile_time_constants`).
/// Used by `serialize_condition` to inline `__platform__ === N`
/// comparisons at codegen time so the emitted ArkTS doesn't reference
/// undeclared identifiers.
///
/// This codegen path is harmonyos-only (the compile.rs harvest at
/// line 1071 only fires on `--target harmonyos[-simulator]`), so
/// `__platform__` is always 9.0 here. The platform-id table lives in
/// `crates/perry-codegen/src/codegen.rs:672-700`.
pub(crate) fn collect_compile_time_constants(init: &[Stmt]) -> HashMap<LocalId, f64> {
    let mut map = HashMap::new();
    for stmt in init {
        if let Stmt::Let {
            id,
            name,
            init: None,
            ..
        } = stmt
        {
            // Mirror codegen.rs::compile_time_constants — only the
            // canonical names are recognized. Anything else is a regular
            // hoisted let binding that resolves through the normal
            // `bindings` map.
            match name.as_str() {
                "__platform__" => {
                    // This codegen is harmonyos-only — see emit_index_ets
                    // call site in crates/perry/src/commands/compile.rs.
                    map.insert(*id, 9.0);
                }
                "__plugins__" => {
                    // No harmonyos-specific plugin set today; default to 0.
                    map.insert(*id, 0.0);
                }
                _ => {}
            }
        }
    }
    map
}

/// Resolve `Expr::LocalGet(id)` to its bound init expression if available.
/// Returns the original expression for any non-LocalGet shape so callers
/// can use it as a transparent identity-or-deref helper.
pub(crate) fn resolve(expr: &Expr, bindings: &HashMap<LocalId, Expr>) -> Expr {
    // Chase chains of LocalGet → LocalGet → ... → real expr. Phase B
    // of the inliner introduces aliasing chains: a top-level
    // `const disconnectBtn = makeDangerBtn(...)` becomes
    // `const disconnectBtn = LocalGet(remapped_btn)` after the call
    // gets inlined and substituted. emit_widget needs to chase past
    // these aliases to find the actual NativeMethodCall(Button, ...).
    // 16-hop cap mirrors numeric_arg_resolved / resolve_string_arg's
    // safety bound.
    let mut cur = expr.clone();
    for _ in 0..16 {
        let next = match &cur {
            Expr::LocalGet(id) => bindings.get(id).cloned(),
            _ => return cur,
        };
        match next {
            Some(e) => cur = e,
            None => return cur,
        }
    }
    cur
}
