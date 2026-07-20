// This module is part of the perry-codegen-arkts crate. It was
// mechanically split out of the former monolithic lib.rs (issue
// #1100). Pure code move — no logic changes.
#![allow(clippy::too_many_arguments)]
use crate::*;

/// Returns true iff every leaf in the expression is either a literal,
/// a compile-time-const LocalGet, or a binding-resolvable LocalGet
/// whose underlying init is itself cleanly serializable. PropertyGets,
/// function calls, and unresolvable LocalGets all return false — those
/// can't be safely interpolated as ArkTS condition source without
/// emitting an undeclared identifier (#410) or a type-mismatched
/// expression like `true.length === 0` (#413 follow-up).
pub(crate) fn is_cleanly_serializable_condition(
    e: &Expr,
    bindings: &HashMap<LocalId, Expr>,
    compile_time_consts: &HashMap<LocalId, f64>,
) -> bool {
    match e {
        Expr::Bool(_) | Expr::Number(_) | Expr::Integer(_) | Expr::String(_) => true,
        Expr::Null | Expr::Undefined => true,
        Expr::LocalGet(id) => {
            if compile_time_consts.contains_key(id) {
                return true;
            }
            match bindings.get(id) {
                Some(init) => {
                    is_cleanly_serializable_condition(init, bindings, compile_time_consts)
                }
                None => false,
            }
        }
        Expr::Compare { left, right, .. } => {
            is_cleanly_serializable_condition(left, bindings, compile_time_consts)
                && is_cleanly_serializable_condition(right, bindings, compile_time_consts)
        }
        Expr::Logical { left, right, .. } => {
            is_cleanly_serializable_condition(left, bindings, compile_time_consts)
                && is_cleanly_serializable_condition(right, bindings, compile_time_consts)
        }
        Expr::Unary { operand, .. } => {
            is_cleanly_serializable_condition(operand, bindings, compile_time_consts)
        }
        // PropertyGet, Call, NativeMethodCall, etc. — can't serialize.
        // Caller falls back to `true` so the conditional always
        // renders its then-branch (matching the v0.5.487 unresolvable-
        // LocalGet heuristic).
        _ => false,
    }
}

pub(crate) fn serialize_condition(
    e: &Expr,
    bindings: &HashMap<LocalId, Expr>,
    compile_time_consts: &HashMap<LocalId, f64>,
) -> String {
    use perry_hir::ir::{CompareOp, LogicalOp};

    // Pessimistic safety gate: if the expression contains anything that
    // can't be cleanly serialized into ArkTS source (PropertyGet on an
    // unresolvable LocalGet, function calls, complex member chains), the
    // current per-node fallbacks would produce gibberish like
    // `true.length === 0` or `true === connectionNames`. ArkTS strict
    // mode rejects both. Degrade the entire condition to `true` (always-
    // render the then-branch) — same heuristic as the unresolvable-
    // LocalGet fallback at the leaf level, just lifted to the root so
    // wrapping shapes (PropertyGet, Comparison-with-non-foldable-side)
    // don't leak.
    if !is_cleanly_serializable_condition(e, bindings, compile_time_consts) {
        return "true".to_string();
    }
    // Wrap a sub-expression's serialized form in parentheses if the
    // sub-expression is a Binary/Logical/Unary shape (post-resolve), so
    // splicing into a parent operator string can't invert precedence.
    // LocalGet recurses through resolution (compile_time_consts then
    // bindings), so we test the *resolved* expression to decide.
    fn needs_parens(e: &Expr, bindings: &HashMap<LocalId, Expr>) -> bool {
        let resolved = match e {
            Expr::LocalGet(id) => bindings.get(id).cloned(),
            _ => None,
        };
        let target = resolved.as_ref().unwrap_or(e);
        matches!(
            target,
            Expr::Compare { .. } | Expr::Logical { .. } | Expr::Unary { .. }
        )
    }
    fn wrap(e: &Expr, bindings: &HashMap<LocalId, Expr>, ts: String) -> String {
        if needs_parens(e, bindings) {
            format!("({})", ts)
        } else {
            ts
        }
    }
    match e {
        Expr::Bool(true) => "true".to_string(),
        Expr::Bool(false) => "false".to_string(),
        Expr::Compare { op, left, right } => {
            let op_str = match op {
                CompareOp::Eq => " === ",
                CompareOp::Ne => " !== ",
                CompareOp::LooseEq => " == ",
                CompareOp::LooseNe => " != ",
                CompareOp::Lt => " < ",
                CompareOp::Le => " <= ",
                CompareOp::Gt => " > ",
                CompareOp::Ge => " >= ",
            };
            let l = serialize_condition(left, bindings, compile_time_consts);
            let r = serialize_condition(right, bindings, compile_time_consts);
            format!(
                "{}{}{}",
                wrap(left, bindings, l),
                op_str,
                wrap(right, bindings, r)
            )
        }
        Expr::Logical { op, left, right } => {
            let op_str = match op {
                LogicalOp::And => " && ",
                LogicalOp::Or => " || ",
                LogicalOp::Coalesce => " ?? ",
            };
            let l = serialize_condition(left, bindings, compile_time_consts);
            let r = serialize_condition(right, bindings, compile_time_consts);
            format!(
                "{}{}{}",
                wrap(left, bindings, l),
                op_str,
                wrap(right, bindings, r)
            )
        }
        Expr::Unary { op, operand } => {
            use perry_hir::ir::UnaryOp;
            let op_str = match op {
                UnaryOp::Not => "!",
                UnaryOp::Neg => "-",
                UnaryOp::Pos => "+",
                UnaryOp::BitNot => "~",
            };
            let inner = serialize_condition(operand, bindings, compile_time_consts);
            format!("{}{}", op_str, wrap(operand, bindings, inner))
        }
        Expr::String(s) => arkts_string_lit(s),
        Expr::Number(n) => fmt_num(*n),
        Expr::Integer(n) => format!("{}", n),
        Expr::LocalGet(id) => {
            // Compile-time platform constants (e.g. `declare const
            // __platform__: number`) are inlined as numeric literals.
            // For the harmonyos codegen path this is always 9.0; the
            // map is populated by `collect_compile_time_constants`.
            if let Some(v) = compile_time_consts.get(id) {
                return fmt_num(*v);
            }
            // Try to resolve through const-bindings. For
            // `let mobile = (__platform__ === 1)` the resolved condition
            // is `(__platform__ === 1)` which then recurses through this
            // same function and inlines the platform literal.
            if let Some(init) = bindings.get(id) {
                return serialize_condition(init, bindings, compile_time_consts);
            }
            // Unresolvable LocalGet — degrade to `true` so the emitted
            // ArkTS compiles cleanly. Conditionality is lost; the
            // mutation always renders as if the predicate were truthy.
            // Emitting `__local_N` here would leak as an undeclared
            // identifier into the page struct (see #410 lines 48/52/68).
            "true".to_string()
        }
        Expr::PropertyGet {
            object, property, ..
        } => {
            // `obj.prop` shape — used commonly in conditions like
            // `props.mobile`. Recursively stringify the object access
            // chain. Keeps the predicate syntactically valid; the
            // user-side reference may not actually exist at the ArkTS
            // page-struct scope, in which case ArkTS's compiler
            // surfaces it as a separate error during emission.
            format!(
                "{}.{}",
                serialize_condition(object, bindings, compile_time_consts),
                property
            )
        }
        // Fallback: emit `true` (literally — no diagnostic comment, since
        // the comment's `*/` would close any wrapping block-comment
        // marker, see #410 line-82 cascade). Conditionality is lost but
        // the build stays green.
        _ => "true".to_string(),
    }
}

/// Issue #413 — try to constant-fold a condition expression. Returns
/// `Some(true)`/`Some(false)` when every operand bottoms out in a
/// literal (Bool/Number/Integer/String/Null/Undefined) and resolves
/// fully through `bindings` and `compile_time_consts`. Returns `None`
/// when any non-literal leaf is reached (e.g. PropertyGet on a runtime
/// value, an unresolved LocalGet, a Call/NativeMethodCall, etc.).
///
/// The caller in `collect_mutations_in_stmt` uses this to drop dead
/// `if` branches at harvest time. Without this, expressions like
/// `__platform__ === 1` (after `__platform__` inlines to 9) would emit
/// as ArkTS `if (9 === 1) { ... }` — which strict-mode ArkTS rejects
/// with a "comparison appears to be unintentional because the types
/// '9' and '1' have no overlap" error. By folding to `Some(false)` and
/// dropping the `if`, we keep the emitted source legal.
pub(crate) fn evaluate_condition(
    e: &Expr,
    bindings: &HashMap<LocalId, Expr>,
    compile_time_consts: &HashMap<LocalId, f64>,
) -> Option<bool> {
    use perry_hir::ir::{CompareOp, LogicalOp, UnaryOp};
    /// Inner repr of a fully-resolved literal value the constant-folder
    /// can reason about. Anything not representable here returns None
    /// from `to_lit` and propagates as the caller's None.
    #[derive(Debug, Clone, PartialEq)]
    enum Lit {
        Bool(bool),
        Num(f64),
        Str(String),
        Null,
        Undefined,
    }
    fn to_lit(
        e: &Expr,
        bindings: &HashMap<LocalId, Expr>,
        compile_time_consts: &HashMap<LocalId, f64>,
    ) -> Option<Lit> {
        match e {
            Expr::Bool(b) => Some(Lit::Bool(*b)),
            Expr::Number(n) => Some(Lit::Num(*n)),
            Expr::Integer(n) => Some(Lit::Num(*n as f64)),
            Expr::String(s) => Some(Lit::Str(s.clone())),
            Expr::Null => Some(Lit::Null),
            Expr::Undefined => Some(Lit::Undefined),
            Expr::LocalGet(id) => {
                if let Some(v) = compile_time_consts.get(id) {
                    return Some(Lit::Num(*v));
                }
                if let Some(init) = bindings.get(id) {
                    return to_lit(init, bindings, compile_time_consts);
                }
                None
            }
            // Known stubbed perry/system + perry/ui functions that
            // return 0 / false on HarmonyOS (the v0.5.477 build.rs
            // auto-stubs all return zero values). Treating them as
            // 0 here makes `dark = isDarkMode()` fold to `dark = 0`
            // at codegen time, which then propagates through
            // `dark ? darkColor : lightColor` to pick the light-mode
            // branch. Without this, the heuristic-pick-then-branch
            // fallback selects darkColor and Mango renders translucent
            // light-on-light text.
            Expr::Call { callee, .. } => match callee.as_ref() {
                Expr::ExternFuncRef { name, .. } if is_harmonyos_zero_fn(name) => {
                    Some(Lit::Num(0.0))
                }
                Expr::FuncRef(_) => None,
                _ => None,
            },
            // perry/system.isDarkMode() may also surface as a
            // NativeMethodCall — same treatment.
            Expr::NativeMethodCall { module, method, .. }
                if module == "perry/system" && is_harmonyos_zero_fn(method) =>
            {
                Some(Lit::Num(0.0))
            }
            Expr::Compare { op, left, right } => {
                let l = to_lit(left, bindings, compile_time_consts)?;
                let r = to_lit(right, bindings, compile_time_consts)?;
                let res = match op {
                    CompareOp::Eq => lit_strict_eq(&l, &r),
                    CompareOp::Ne => !lit_strict_eq(&l, &r),
                    CompareOp::LooseEq => lit_loose_eq(&l, &r),
                    CompareOp::LooseNe => !lit_loose_eq(&l, &r),
                    CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge => {
                        let (Lit::Num(a), Lit::Num(b)) = (&l, &r) else {
                            return None;
                        };
                        match op {
                            CompareOp::Lt => a < b,
                            CompareOp::Le => a <= b,
                            CompareOp::Gt => a > b,
                            CompareOp::Ge => a >= b,
                            _ => unreachable!(),
                        }
                    }
                };
                Some(Lit::Bool(res))
            }
            Expr::Logical { op, left, right } => {
                let l = to_lit(left, bindings, compile_time_consts)?;
                match op {
                    LogicalOp::And => {
                        if !lit_truthy(&l) {
                            Some(l)
                        } else {
                            to_lit(right, bindings, compile_time_consts)
                        }
                    }
                    LogicalOp::Or => {
                        if lit_truthy(&l) {
                            Some(l)
                        } else {
                            to_lit(right, bindings, compile_time_consts)
                        }
                    }
                    LogicalOp::Coalesce => {
                        if matches!(l, Lit::Null | Lit::Undefined) {
                            to_lit(right, bindings, compile_time_consts)
                        } else {
                            Some(l)
                        }
                    }
                }
            }
            Expr::Unary { op, operand } => {
                let v = to_lit(operand, bindings, compile_time_consts)?;
                match op {
                    UnaryOp::Not => Some(Lit::Bool(!lit_truthy(&v))),
                    UnaryOp::Neg => match v {
                        Lit::Num(n) => Some(Lit::Num(-n)),
                        _ => None,
                    },
                    UnaryOp::Pos => match v {
                        Lit::Num(n) => Some(Lit::Num(n)),
                        _ => None,
                    },
                    UnaryOp::BitNot => match v {
                        Lit::Num(n) => Some(Lit::Num((!(n as i32)) as f64)),
                        _ => None,
                    },
                }
            }
            _ => None,
        }
    }
    fn lit_truthy(l: &Lit) -> bool {
        match l {
            Lit::Bool(b) => *b,
            Lit::Num(n) => *n != 0.0 && !n.is_nan(),
            Lit::Str(s) => !s.is_empty(),
            Lit::Null | Lit::Undefined => false,
        }
    }
    fn lit_strict_eq(a: &Lit, b: &Lit) -> bool {
        match (a, b) {
            (Lit::Bool(x), Lit::Bool(y)) => x == y,
            (Lit::Num(x), Lit::Num(y)) => x == y,
            (Lit::Str(x), Lit::Str(y)) => x == y,
            (Lit::Null, Lit::Null) => true,
            (Lit::Undefined, Lit::Undefined) => true,
            _ => false,
        }
    }
    fn lit_loose_eq(a: &Lit, b: &Lit) -> bool {
        // `null == undefined` per spec, plus strict-eq for matching kinds.
        // Cross-type numeric/string coercion is intentionally not
        // implemented here — we only resolve the cases we can do safely.
        match (a, b) {
            (Lit::Null, Lit::Undefined) | (Lit::Undefined, Lit::Null) => true,
            _ => lit_strict_eq(a, b),
        }
    }
    let l = to_lit(e, bindings, compile_time_consts)?;
    Some(lit_truthy(&l))
}
