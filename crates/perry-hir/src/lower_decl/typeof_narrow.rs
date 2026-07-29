//! `typeof x === "<primitive>"` control-flow narrowing for `if`-stmt branches
//! (#2277).
//!
//! Detects the AST-level shape `typeof <ident> === "<lit>"` (and the negated
//! `!==` form) and pushes a per-branch shadow binding into the lowering
//! context's `locals` stack so the dispatcher (`local_array_methods.rs`'s
//! `is_known_string` / `is_union_with_string`, etc.) sees the post-narrowing
//! type. The shadow is removed automatically by the `pop_block_scope` mark
//! the if-statement lowering already takes.
//!
//! Pre-fix the narrowing only happened to "work" for unions whose members
//! shared the method being called (e.g. `string | number | boolean`'s
//! `.toString()` lives on every variant). For unions with disjoint method
//! surfaces — the canonical case being `string | T[]` and `.join(",")` on
//! the else branch — `is_union_with_string` flipped to true and dispatch
//! routed `.join` through the string-method path, surfacing as
//! `TypeError: (string).join is not a function`.
//!
//! Scope is narrow on purpose: only primitive typeof comparisons against a
//! string literal of one of the seven unambiguous JS typeof tags are
//! handled. `"function"` and `"object"` are excluded — their narrowed-to
//! types (`Function(_)` / `Object(_)`/`Array(_)`/`Named(_)`) aren't a single
//! variant in our `Type` enum and naive narrowing would either lose
//! precision or incorrectly drop array methods from `object`-typed unions.

use crate::types::Type;
use anyhow::Result;
use swc_ecma_ast as ast;

use crate::ir::Stmt;
use crate::lower::{lower_expr, LoweringContext};

/// Result of pattern-matching the if-statement test against the
/// `typeof <ident> {===,!==} "<primitive>"` shape.
#[derive(Debug)]
pub(crate) struct TypeofGuard<'a> {
    pub var_name: &'a str,
    pub typeof_str: &'a str,
    /// True for `!==` (then-branch is the EXCLUDE side, else is INCLUDE)
    /// and for the swapped operand order; false for the canonical
    /// `typeof x === "lit"` (then = INCLUDE, else = EXCLUDE).
    pub negated: bool,
}

/// Recognize `typeof x === "lit"` / `"lit" === typeof x` and the negated
/// forms. Returns `None` for everything else.
pub(crate) fn extract_typeof_guard(cond: &ast::Expr) -> Option<TypeofGuard<'_>> {
    let bin = match cond {
        ast::Expr::Bin(b) => b,
        ast::Expr::Paren(p) => return extract_typeof_guard(&p.expr),
        _ => return None,
    };

    let op_negated = match bin.op {
        ast::BinaryOp::EqEqEq | ast::BinaryOp::EqEq => false,
        ast::BinaryOp::NotEqEq | ast::BinaryOp::NotEq => true,
        _ => return None,
    };

    let (var_name, typeof_str) = try_extract_typeof_pair(&bin.left, &bin.right)
        .or_else(|| try_extract_typeof_pair(&bin.right, &bin.left))?;

    Some(TypeofGuard {
        var_name,
        typeof_str,
        negated: op_negated,
    })
}

fn try_extract_typeof_pair<'a>(
    lhs: &'a ast::Expr,
    rhs: &'a ast::Expr,
) -> Option<(&'a str, &'a str)> {
    let unary = match lhs {
        ast::Expr::Unary(u) => u,
        ast::Expr::Paren(p) => match p.expr.as_ref() {
            ast::Expr::Unary(u) => u,
            _ => return None,
        },
        _ => return None,
    };
    if !matches!(unary.op, ast::UnaryOp::TypeOf) {
        return None;
    }
    let var = match unary.arg.as_ref() {
        ast::Expr::Ident(id) => id.sym.as_ref(),
        ast::Expr::Paren(p) => match p.expr.as_ref() {
            ast::Expr::Ident(id) => id.sym.as_ref(),
            _ => return None,
        },
        _ => return None,
    };
    let typeof_str = match rhs {
        ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str().unwrap_or(""),
        ast::Expr::Paren(p) => match p.expr.as_ref() {
            ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str().unwrap_or(""),
            _ => return None,
        },
        _ => return None,
    };
    Some((var, typeof_str))
}

/// Map a `typeof` string tag to the corresponding HIR `Type` variant for
/// narrowing purposes. Only the unambiguous primitive tags are mapped —
/// `"function"` / `"object"` widen too many variants for a cheap
/// shadow-narrowing to model correctly, so they fall through to the
/// pre-fix behaviour.
fn type_for_typeof_str(typeof_str: &str) -> Option<Type> {
    match typeof_str {
        "string" => Some(Type::String),
        "number" => Some(Type::Number),
        "boolean" => Some(Type::Boolean),
        "bigint" => Some(Type::BigInt),
        "undefined" => Some(Type::Void),
        "symbol" => Some(Type::Symbol),
        _ => None,
    }
}

/// Return the *then-branch* narrowed type after the if-stmt condition
/// asserts `typeof <var> === "<typeof_str>"`. Returns `None` when there
/// is no useful narrowing to apply (so the caller leaves the binding's
/// declared type alone).
pub(crate) fn narrowed_then(orig: &Type, typeof_str: &str) -> Option<Type> {
    let target = type_for_typeof_str(typeof_str)?;
    match orig {
        Type::Union(variants) => {
            // Pick the variant(s) that match `target`. For "string" against
            // `Union(String, Array(Number))` this lands on `String`.
            let matches: Vec<Type> = variants
                .iter()
                .filter(|v| variant_matches_typeof(v, &target))
                .cloned()
                .collect();
            match matches.len() {
                0 => None, // Union doesn't contain the target — no narrowing.
                1 => Some(matches.into_iter().next().unwrap()),
                _ => Some(Type::Union(matches)),
            }
        }
        // Already the target type — no shadow needed.
        _ => None,
    }
}

/// Return the *else-branch* narrowed type after the if-stmt condition
/// asserts `typeof <var> === "<typeof_str>"`. Mirrors TypeScript's
/// `Exclude<T, <typeof'd type>>`. Returns `None` when no useful narrowing.
pub(crate) fn narrowed_else(orig: &Type, typeof_str: &str) -> Option<Type> {
    let target = type_for_typeof_str(typeof_str)?;
    match orig {
        Type::Union(variants) => {
            let remainder: Vec<Type> = variants
                .iter()
                .filter(|v| !variant_matches_typeof(v, &target))
                .cloned()
                .collect();
            match remainder.len() {
                0 => None, // Every variant matched — else branch unreachable; leave decl.
                n if n == variants.len() => None, // Nothing changed — no shadow needed.
                1 => Some(remainder.into_iter().next().unwrap()),
                _ => Some(Type::Union(remainder)),
            }
        }
        _ => None,
    }
}

/// Whether a union variant matches a `typeof` tag (i.e., would be
/// narrowed away on the corresponding branch). Conservative — only the
/// primitive variants are eligible because that's the safe set the
/// guard tag-string can prove.
fn variant_matches_typeof(variant: &Type, target: &Type) -> bool {
    match (variant, target) {
        (Type::String, Type::String) => true,
        (Type::Number | Type::Int32, Type::Number) => true,
        (Type::Boolean, Type::Boolean) => true,
        (Type::BigInt, Type::BigInt) => true,
        (Type::Void | Type::Null, Type::Void) => false, // `typeof null === "object"` in JS
        (Type::Symbol, Type::Symbol) => true,
        _ => false,
    }
}

/// Lower a single `if`-statement with typeof control-flow narrowing.
/// Recognizes the AST-level guard, then-lowers / else-lowers each
/// branch with the appropriate shadow binding pushed onto
/// `ctx.locals`. The shadow is removed when the branch's
/// `pop_block_scope` mark unwinds. Caller passes the body-stmt
/// lowerer as a function pointer so the helper doesn't depend on
/// `lower_decl/body_stmt.rs`'s private items.
pub(crate) fn lower_if_with_narrowing(
    ctx: &mut LoweringContext,
    if_stmt: &ast::IfStmt,
    lower_body: fn(&mut LoweringContext, &ast::Stmt) -> Result<Vec<Stmt>>,
) -> Result<Stmt> {
    let condition = lower_expr(ctx, &if_stmt.test)?;
    let guard = extract_typeof_guard(&if_stmt.test);

    let then_branch = {
        let mark = ctx.push_block_scope();
        push_typeof_narrow_shadow(ctx, guard.as_ref(), /* else_side= */ false);
        let stmts = lower_body(ctx, &if_stmt.cons)?;
        ctx.pop_block_scope(mark);
        stmts
    };

    let else_branch = if_stmt
        .alt
        .as_ref()
        .map(|s| -> Result<Vec<Stmt>> {
            let mark = ctx.push_block_scope();
            push_typeof_narrow_shadow(ctx, guard.as_ref(), /* else_side= */ true);
            let stmts = lower_body(ctx, s)?;
            ctx.pop_block_scope(mark);
            Ok(stmts)
        })
        .transpose()?;

    Ok(Stmt::If {
        condition,
        then_branch,
        else_branch,
    })
}

/// Push a shadow `(name, id, narrowed_type)` entry onto the lowering
/// context's `locals` stack so `lookup_local_type(name)` returns the
/// post-typeof-narrow type for the duration of the if-branch. The
/// shadow is removed automatically by the `pop_block_scope` mark the
/// caller already takes.
///
/// Skips quietly when (a) no guard was extracted, (b) the typeof tag
/// doesn't map to a narrowable variant, (c) the var name doesn't
/// resolve, (d) the original type isn't a union, or (e) the union
/// doesn't change after narrowing. All five cases mean "leave the
/// existing binding visible," which preserves pre-#2277 behaviour.
pub(crate) fn push_typeof_narrow_shadow(
    ctx: &mut LoweringContext,
    guard: Option<&TypeofGuard<'_>>,
    else_side: bool,
) {
    let Some(g) = guard else { return };
    let Some(local_id) = ctx.lookup_local(g.var_name) else {
        return;
    };
    let orig = match ctx.lookup_local_type(g.var_name) {
        Some(ty) => ty.clone(),
        None => return,
    };
    // `typeof x === "string"`: then-side gets the include narrow,
    // else-side gets the exclude. `typeof x !== "string"` swaps it.
    let want_exclude = else_side ^ g.negated;
    let narrowed = if want_exclude {
        narrowed_else(&orig, g.typeof_str)
    } else {
        narrowed_then(&orig, g.typeof_str)
    };
    let Some(narrowed) = narrowed else {
        return;
    };
    ctx.locals
        .push((g.var_name.to_string(), local_id, narrowed));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ty_union(variants: Vec<Type>) -> Type {
        Type::Union(variants)
    }

    #[test]
    fn else_branch_excludes_string_from_string_or_array_union() {
        let orig = ty_union(vec![Type::String, Type::Array(Box::new(Type::Number))]);
        let narrowed = narrowed_else(&orig, "string").expect("must narrow");
        assert_eq!(narrowed, Type::Array(Box::new(Type::Number)));
    }

    #[test]
    fn else_branch_excludes_string_from_three_way_union() {
        let orig = ty_union(vec![Type::String, Type::Number, Type::Boolean]);
        let narrowed = narrowed_else(&orig, "string").expect("must narrow");
        assert_eq!(narrowed, ty_union(vec![Type::Number, Type::Boolean]));
    }

    #[test]
    fn then_branch_picks_string_from_union() {
        let orig = ty_union(vec![Type::String, Type::Array(Box::new(Type::Number))]);
        let narrowed = narrowed_then(&orig, "string").expect("must narrow");
        assert_eq!(narrowed, Type::String);
    }

    #[test]
    fn non_union_type_returns_none() {
        let orig = Type::String;
        assert!(narrowed_then(&orig, "string").is_none());
        assert!(narrowed_else(&orig, "string").is_none());
    }

    #[test]
    fn unsupported_typeof_tag_returns_none() {
        let orig = ty_union(vec![Type::String, Type::Array(Box::new(Type::Number))]);
        assert!(narrowed_then(&orig, "function").is_none());
        assert!(narrowed_else(&orig, "object").is_none());
    }
}
