//! Pattern and literal lowering utilities.
//!
//! Contains functions for lowering literals, assignment targets, binding names,
//! parameter destructuring, and other pattern-related utilities.

use crate::ir::*;
use crate::lower::{lower_expr, LoweringContext};
use crate::lower_types::*;
use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_common::Spanned;
use swc_ecma_ast as ast;

pub(crate) fn unescape_template(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('r') => result.push('\r'),
                Some('b') => result.push('\u{0008}'),
                Some('f') => result.push('\u{000C}'),
                Some('v') => result.push('\u{000B}'),
                Some('\\') => result.push('\\'),
                Some('$') => result.push('$'),
                Some('`') => result.push('`'),
                Some('\'') => result.push('\''),
                Some('"') => result.push('"'),
                // `\0` (not followed by another digit) is NUL.
                Some('0') if !chars.peek().is_some_and(|d| d.is_ascii_digit()) => result.push('\0'),
                // Line continuation: backslash-newline contributes nothing.
                Some('\n') => {}
                Some('\r') => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                }
                // `\xHH` / `\uHHHH` / `\u{H…}` — #5039: ansi-styles builds its
                // escape codes as `` `\u001B[${code}m` `` template literals;
                // falling through to the literal-backslash arm turned every
                // chalk style into the 6-char literal source text instead of ESC.
                Some(esc @ ('x' | 'u')) => {
                    if let Some(decoded) = unescape_hex_escape(esc, &mut chars) {
                        result.push_str(&decoded);
                    } else {
                        // Invalid escape (only reachable in tagged templates,
                        // where cooked semantics are undefined) — keep the
                        // original text.
                        result.push('\\');
                        result.push(esc);
                    }
                }
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Decode the body of a `\xHH`, `\uHHHH`, or `\u{H…}` escape, with `esc`
/// being the introducer character just consumed (`x` or `u`). A `\uD800–DBFF`
/// high surrogate followed immediately by an escaped low surrogate decodes as
/// the combined supplementary code point; a lone surrogate becomes U+FFFD
/// (Perry strings are UTF-8 — see the WTF-8 categorical gap in CLAUDE.md).
/// Returns `None` (consuming nothing further) on malformed hex so the caller
/// can preserve the source text.
fn unescape_hex_escape(
    esc: char,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Option<String> {
    fn hex_fixed(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, n: usize) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..n {
            let d = chars.peek()?.to_digit(16)?;
            chars.next();
            value = value * 16 + d;
        }
        Some(value)
    }
    fn hex_braced(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<u32> {
        chars.next(); // consume '{'
        let mut value = 0u32;
        let mut any = false;
        loop {
            match chars.peek() {
                Some('}') => {
                    chars.next();
                    return any.then_some(value);
                }
                Some(c) => {
                    let d = c.to_digit(16)?;
                    chars.next();
                    any = true;
                    value = value.checked_mul(16)?.checked_add(d)?;
                    if value > 0x10FFFF {
                        return None;
                    }
                }
                None => return None,
            }
        }
    }

    let code = if esc == 'x' {
        hex_fixed(chars, 2)?
    } else if chars.peek() == Some(&'{') {
        hex_braced(chars)?
    } else {
        hex_fixed(chars, 4)?
    };

    // High surrogate: try to pair with an immediately following `\uDC00–DFFF`.
    if (0xD800..=0xDBFF).contains(&code) {
        let mut lookahead = chars.clone();
        if lookahead.next() == Some('\\') && lookahead.next() == Some('u') {
            if let Some(low) = hex_fixed(&mut lookahead, 4) {
                if (0xDC00..=0xDFFF).contains(&low) {
                    *chars = lookahead;
                    let combined = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                    return char::from_u32(combined).map(String::from);
                }
            }
        }
    }

    Some(char::from_u32(code).unwrap_or('\u{FFFD}').to_string())
}

pub(crate) fn lower_lit(lit: &ast::Lit) -> Result<Expr> {
    match lit {
        ast::Lit::Num(n) => {
            let value = n.value;
            // Check if this is an integer that fits in i64
            if value.fract() == 0.0 && value >= i64::MIN as f64 && value <= i64::MAX as f64 {
                Ok(Expr::Integer(value as i64))
            } else {
                Ok(Expr::Number(value))
            }
        }
        ast::Lit::Str(s) => {
            if let Some(valid_utf8) = s.value.as_str() {
                Ok(Expr::String(normalize_swc_string_literal(s, valid_utf8)))
            } else {
                // Lone surrogates (U+D800..U+DFFF): SWC stores them as WTF-8 bytes.
                // as_str() returns None because they can't be represented as valid UTF-8.
                Ok(Expr::WtfString(s.value.as_bytes().to_vec()))
            }
        }
        ast::Lit::Bool(b) => Ok(Expr::Bool(b.value)),
        ast::Lit::Null(_) => Ok(Expr::Null),
        ast::Lit::BigInt(bi) => Ok(Expr::BigInt(bi.value.to_string())),
        ast::Lit::Regex(re) => Ok(Expr::RegExp {
            pattern: re.exp.to_string(),
            flags: re.flags.to_string(),
        }),
        _ => Err(anyhow!("Unsupported literal type")),
    }
}

fn normalize_swc_string_literal(lit: &ast::Str, decoded: &str) -> String {
    let Some(raw) = lit.raw.as_ref().map(|raw| raw.as_ref()) else {
        return decoded.to_string();
    };
    if raw.is_ascii() || decoded.is_ascii() {
        return decoded.to_string();
    }
    let mut bytes = Vec::with_capacity(decoded.len());
    for ch in decoded.chars() {
        let code = ch as u32;
        if code > 0xFF {
            return decoded.to_string();
        }
        bytes.push(code as u8);
    }
    match String::from_utf8(bytes) {
        Ok(repaired) => repaired,
        Err(_) => decoded.to_string(),
    }
}

/// Convert an assignment target to an expression for reading its current value
/// Used for compound assignment operators like += to read the current value before modifying
pub(crate) fn lower_assign_target_to_expr(
    ctx: &mut LoweringContext,
    target: &ast::AssignTarget,
) -> Result<Expr> {
    match target {
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(ident)) => {
            let name = ident.id.sym.to_string();
            if let Some(id) = ctx.lookup_local(&name) {
                Ok(Expr::LocalGet(id))
            } else {
                // Unresolved / global compound-assignment target. The current
                // value is obtained via GetValue on the reference, so the read
                // must follow the same resolution as a bare identifier read:
                // known globals resolve to their value, while a truly
                // unresolvable name lowers to a runtime ReferenceError throw
                // (GetValue on an unresolvable Reference always throws, in both
                // strict and sloppy mode — e.g. `x *= 1` with `x` undeclared).
                // Previously this hard-errored at compile time, turning a
                // catchable ReferenceError into a SyntaxError.
                lower_expr(ctx, &ast::Expr::Ident(ident.id.clone()))
            }
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Member(member)) => {
            // Check if this is a static field access
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                let obj_name = obj_ident.sym.to_string();
                if ctx.lookup_class(&obj_name).is_some() {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        let field_name = prop_ident.sym.to_string();
                        if ctx.has_static_field(&obj_name, &field_name) {
                            return Ok(Expr::StaticFieldGet {
                                class_name: obj_name,
                                field_name,
                            });
                        }
                    }
                }
            }

            let object = Box::new(lower_expr(ctx, &member.obj)?);
            match &member.prop {
                ast::MemberProp::Ident(ident) => {
                    let property = ident.sym.to_string();
                    Ok(Expr::PropertyGet { object, property })
                }
                ast::MemberProp::Computed(computed) => {
                    let index = Box::new(lower_expr(ctx, &computed.expr)?);
                    Ok(Expr::IndexGet { object, index })
                }
                ast::MemberProp::PrivateName(private) => {
                    let property = format!("#{}", private.name);
                    Ok(Expr::PropertyGet { object, property })
                }
            }
        }
        // Unwrap TypeScript type annotations and parentheses to get the real target
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Paren(paren)) => {
            lower_expr(ctx, &paren.expr)
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsAs(ts_as)) => {
            lower_expr(ctx, &ts_as.expr)
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsNonNull(ts_nn)) => {
            lower_expr(ctx, &ts_nn.expr)
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsTypeAssertion(ts_ta)) => {
            lower_expr(ctx, &ts_ta.expr)
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsSatisfies(ts_sat)) => {
            lower_expr(ctx, &ts_sat.expr)
        }
        _ => Err(anyhow!("Unsupported target in compound assignment")),
    }
}

pub(crate) fn get_binding_name(pat: &ast::Pat) -> Result<String> {
    match pat {
        ast::Pat::Ident(ident) => Ok(ident.id.sym.to_string()),
        _ => {
            crate::lower_bail!(pat.span(), "Unsupported binding pattern");
        }
    }
}

pub(crate) fn collect_binding_names(pat: &ast::Pat, out: &mut Vec<String>) {
    match pat {
        ast::Pat::Ident(ident) => push_unique_name(out, ident.id.sym.to_string()),
        ast::Pat::Array(arr) => {
            for elem in arr.elems.iter().flatten() {
                collect_binding_names(elem, out);
            }
        }
        ast::Pat::Object(obj) => {
            for prop in &obj.props {
                match prop {
                    ast::ObjectPatProp::Assign(assign) => {
                        push_unique_name(out, assign.key.sym.to_string());
                    }
                    ast::ObjectPatProp::KeyValue(kv) => collect_binding_names(&kv.value, out),
                    ast::ObjectPatProp::Rest(rest) => collect_binding_names(&rest.arg, out),
                }
            }
        }
        ast::Pat::Assign(assign) => collect_binding_names(&assign.left, out),
        ast::Pat::Rest(rest) => collect_binding_names(&rest.arg, out),
        ast::Pat::Expr(_) | ast::Pat::Invalid(_) => {}
    }
}

/// Static counter for generating unique synthetic names for destructuring patterns
static DESTRUCT_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub(crate) fn get_pat_name(pat: &ast::Pat) -> Result<String> {
    match pat {
        ast::Pat::Ident(ident) => Ok(ident.id.sym.to_string()),
        ast::Pat::Assign(assign) => get_pat_name(&assign.left),
        ast::Pat::Rest(rest) => get_pat_name(&rest.arg),
        // For complex destructuring patterns, generate synthetic names
        // The actual destructuring will be handled at the call site or as a separate pass
        ast::Pat::Array(_) => {
            let id = DESTRUCT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(format!("__arr_destruct_{}", id))
        }
        ast::Pat::Object(_) => {
            let id = DESTRUCT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(format!("__obj_destruct_{}", id))
        }
        _ => Err(anyhow!("Unsupported pattern")),
    }
}

/// Extract the type annotation from a Pat (for arrow function parameters)
pub(crate) fn get_pat_type(pat: &ast::Pat, ctx: &LoweringContext) -> Type {
    match pat {
        ast::Pat::Ident(ident) => ident
            .type_ann
            .as_ref()
            .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, Some(ctx)))
            .unwrap_or(Type::Any),
        ast::Pat::Assign(assign) => get_pat_type(&assign.left, ctx),
        ast::Pat::Rest(rest) => rest
            .type_ann
            .as_ref()
            .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, Some(ctx)))
            .unwrap_or(Type::Any),
        ast::Pat::Array(arr) => arr
            .type_ann
            .as_ref()
            .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, Some(ctx)))
            .unwrap_or(Type::Any),
        ast::Pat::Object(obj) => obj
            .type_ann
            .as_ref()
            .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, Some(ctx)))
            .unwrap_or(Type::Any),
        _ => Type::Any,
    }
}

/// Generate Let statements to extract destructured variables from a synthetic parameter.
/// For array patterns like `[a, b]`, generates:
///   let a = param[0];
///   let b = param[1];
/// For object patterns like `{a, b}`, generates:
///   let a = param.a;
///   let b = param.b;
/// Delegates to the recursive `lower_pattern_binding` helper so that nested
/// patterns, defaults, rest, and computed keys all work consistently.
pub(crate) fn generate_param_destructuring_stmts(
    ctx: &mut LoweringContext,
    pat: &ast::Pat,
    param_id: LocalId,
) -> Result<Vec<Stmt>> {
    match pat {
        ast::Pat::Array(_) | ast::Pat::Object(_) => {
            // Function parameters are mutable bindings (like `let`), so the
            // destructured locals must be mutable too — JS lets you reassign a
            // destructured param (`([a, b]) => { b -= 1 }`). Passing `false`
            // here marked them `const` and made any such reassignment throw
            // "Assignment to constant variable" (hit by Hono's RegExpRouter).
            crate::destructuring::lower_pattern_binding(
                ctx,
                pat,
                Expr::LocalGet(param_id),
                true,
                // Function params are lexical bindings, not `var` declarations.
                false,
            )
        }
        ast::Pat::Rest(rest) if is_destructuring_pattern(&rest.arg) => {
            crate::destructuring::lower_pattern_binding(
                ctx,
                &rest.arg,
                Expr::LocalGet(param_id),
                true,
                false,
            )
        }
        _ => Ok(Vec::new()),
    }
}

/// Check if a pattern is a destructuring pattern (array or object)
pub(crate) fn is_destructuring_pattern(pat: &ast::Pat) -> bool {
    match pat {
        ast::Pat::Array(_) | ast::Pat::Object(_) => true,
        ast::Pat::Rest(rest) => is_destructuring_pattern(&rest.arg),
        _ => false,
    }
}

fn push_unique_name(names: &mut Vec<String>, name: String) {
    if !names.iter().any(|existing| existing == &name) {
        names.push(name);
    }
}

fn should_predeclare_implicit_assignment_name(ctx: &LoweringContext, name: &str) -> bool {
    ctx.lookup_class(name).is_none()
        && ctx.lookup_func(name).is_none()
        && (ctx.lookup_local(name).is_none() || ctx.pre_registered_module_var_decls.contains(name))
}

fn collect_implicit_assignment_pat_names(
    ctx: &LoweringContext,
    pat: &ast::Pat,
    names: &mut Vec<String>,
) {
    match pat {
        ast::Pat::Ident(ident) => {
            let name = ident.id.sym.to_string();
            if should_predeclare_implicit_assignment_name(ctx, &name) {
                push_unique_name(names, name);
            }
        }
        ast::Pat::Array(arr) => {
            for elem in arr.elems.iter().flatten() {
                collect_implicit_assignment_pat_names(ctx, elem, names);
            }
        }
        ast::Pat::Object(obj) => {
            for prop in &obj.props {
                match prop {
                    ast::ObjectPatProp::Assign(assign) => {
                        let name = assign.key.sym.to_string();
                        if should_predeclare_implicit_assignment_name(ctx, &name) {
                            push_unique_name(names, name);
                        }
                    }
                    ast::ObjectPatProp::KeyValue(kv) => {
                        collect_implicit_assignment_pat_names(ctx, &kv.value, names);
                    }
                    ast::ObjectPatProp::Rest(rest) => {
                        collect_implicit_assignment_pat_names(ctx, &rest.arg, names);
                    }
                }
            }
        }
        ast::Pat::Assign(assign) => {
            collect_implicit_assignment_pat_names(ctx, &assign.left, names);
        }
        ast::Pat::Rest(rest) => {
            collect_implicit_assignment_pat_names(ctx, &rest.arg, names);
        }
        ast::Pat::Expr(_) | ast::Pat::Invalid(_) => {}
    }
}

fn collect_implicit_assignment_target_names(
    ctx: &LoweringContext,
    target: &ast::AssignTarget,
    names: &mut Vec<String>,
) {
    match target {
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(ident)) => {
            let name = ident.id.sym.to_string();
            // Only a pre-registered hoisted `var` is predeclared here — this is
            // its var-hoisting materialization, so `index = index` before the
            // `for (var index …)` reads the hoisted `undefined` instead of a
            // throwing global read (S12.6.3_A10). A GENUINELY-new sloppy global
            // (`foo = 1` with no `var` anywhere) is deliberately NOT predeclared:
            // it lowers to a globalThis property set (#3575), and a backing local
            // would make `lower_expr_assignment` emit `LocalSet`, hiding the write
            // from globalThis. Destructuring targets below always predeclare
            // because their lowering only emits `LocalSet`.
            if ctx.pre_registered_module_var_decls.contains(&name)
                && should_predeclare_implicit_assignment_name(ctx, &name)
            {
                push_unique_name(names, name);
            }
        }
        ast::AssignTarget::Pat(pat) => match pat {
            ast::AssignTargetPat::Array(arr) => {
                for elem in arr.elems.iter().flatten() {
                    collect_implicit_assignment_pat_names(ctx, elem, names);
                }
            }
            ast::AssignTargetPat::Object(obj) => {
                for prop in &obj.props {
                    match prop {
                        ast::ObjectPatProp::Assign(assign) => {
                            let name = assign.key.sym.to_string();
                            if should_predeclare_implicit_assignment_name(ctx, &name) {
                                push_unique_name(names, name);
                            }
                        }
                        ast::ObjectPatProp::KeyValue(kv) => {
                            collect_implicit_assignment_pat_names(ctx, &kv.value, names);
                        }
                        ast::ObjectPatProp::Rest(rest) => {
                            collect_implicit_assignment_pat_names(ctx, &rest.arg, names);
                        }
                    }
                }
            }
            ast::AssignTargetPat::Invalid(_) => {}
        },
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Paren(paren)) => {
            collect_implicit_assignment_expr_names(ctx, &paren.expr, names);
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsAs(ts_as)) => {
            collect_implicit_assignment_expr_names(ctx, &ts_as.expr, names);
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsNonNull(ts_nn)) => {
            collect_implicit_assignment_expr_names(ctx, &ts_nn.expr, names);
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsTypeAssertion(ts_ta)) => {
            collect_implicit_assignment_expr_names(ctx, &ts_ta.expr, names);
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsSatisfies(ts_sat)) => {
            collect_implicit_assignment_expr_names(ctx, &ts_sat.expr, names);
        }
        ast::AssignTarget::Simple(
            ast::SimpleAssignTarget::Member(_)
            | ast::SimpleAssignTarget::SuperProp(_)
            | ast::SimpleAssignTarget::OptChain(_)
            | ast::SimpleAssignTarget::TsInstantiation(_)
            | ast::SimpleAssignTarget::Invalid(_),
        ) => {}
    }
}

fn simple_assign_target_ident_name(target: &ast::AssignTarget) -> Option<&str> {
    match target {
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(ident)) => {
            Some(ident.id.sym.as_ref())
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Paren(paren)) => {
            expr_ident_name(paren.expr.as_ref())
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsAs(ts_as)) => {
            expr_ident_name(ts_as.expr.as_ref())
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsNonNull(ts_nn)) => {
            expr_ident_name(ts_nn.expr.as_ref())
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsTypeAssertion(ts_ta)) => {
            expr_ident_name(ts_ta.expr.as_ref())
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsSatisfies(ts_sat)) => {
            expr_ident_name(ts_sat.expr.as_ref())
        }
        _ => None,
    }
}

fn expr_ident_name(expr: &ast::Expr) -> Option<&str> {
    match expr {
        ast::Expr::Ident(ident) => Some(ident.sym.as_ref()),
        ast::Expr::Paren(paren) => expr_ident_name(paren.expr.as_ref()),
        ast::Expr::TsAs(ts_as) => expr_ident_name(ts_as.expr.as_ref()),
        ast::Expr::TsNonNull(ts_nn) => expr_ident_name(ts_nn.expr.as_ref()),
        ast::Expr::TsTypeAssertion(ts_ta) => expr_ident_name(ts_ta.expr.as_ref()),
        ast::Expr::TsSatisfies(ts_sat) => expr_ident_name(ts_sat.expr.as_ref()),
        _ => None,
    }
}

fn direct_self_read_assignment_name(expr: &ast::Expr) -> Option<&str> {
    match expr {
        ast::Expr::Assign(assign) => simple_assign_target_ident_name(&assign.left)
            .zip(expr_ident_name(assign.right.as_ref()))
            .and_then(|(left, right)| (left == right).then_some(left)),
        ast::Expr::Paren(paren) => direct_self_read_assignment_name(paren.expr.as_ref()),
        ast::Expr::TsAs(ts_as) => direct_self_read_assignment_name(ts_as.expr.as_ref()),
        ast::Expr::TsNonNull(ts_nn) => direct_self_read_assignment_name(ts_nn.expr.as_ref()),
        ast::Expr::TsTypeAssertion(ts_ta) => direct_self_read_assignment_name(ts_ta.expr.as_ref()),
        ast::Expr::TsSatisfies(ts_sat) => direct_self_read_assignment_name(ts_sat.expr.as_ref()),
        _ => None,
    }
}

fn collect_implicit_assignment_expr_names(
    ctx: &LoweringContext,
    expr: &ast::Expr,
    names: &mut Vec<String>,
) {
    match expr {
        ast::Expr::Assign(assign) => {
            if assign.op == ast::AssignOp::Assign {
                let self_read = simple_assign_target_ident_name(&assign.left)
                    .zip(expr_ident_name(assign.right.as_ref()))
                    .is_some_and(|(left, right)| left == right);
                if !self_read {
                    collect_implicit_assignment_target_names(ctx, &assign.left, names);
                }
            }
            collect_implicit_assignment_expr_names(ctx, &assign.right, names);
        }
        ast::Expr::Seq(seq) => {
            for expr in &seq.exprs {
                collect_implicit_assignment_expr_names(ctx, expr, names);
            }
        }
        ast::Expr::Paren(paren) => collect_implicit_assignment_expr_names(ctx, &paren.expr, names),
        ast::Expr::TsAs(ts_as) => collect_implicit_assignment_expr_names(ctx, &ts_as.expr, names),
        ast::Expr::TsNonNull(ts_nn) => {
            collect_implicit_assignment_expr_names(ctx, &ts_nn.expr, names)
        }
        ast::Expr::TsTypeAssertion(ts_ta) => {
            collect_implicit_assignment_expr_names(ctx, &ts_ta.expr, names)
        }
        ast::Expr::TsSatisfies(ts_sat) => {
            collect_implicit_assignment_expr_names(ctx, &ts_sat.expr, names)
        }
        ast::Expr::Bin(bin) => {
            collect_implicit_assignment_expr_names(ctx, &bin.left, names);
            collect_implicit_assignment_expr_names(ctx, &bin.right, names);
        }
        ast::Expr::Unary(unary) => collect_implicit_assignment_expr_names(ctx, &unary.arg, names),
        ast::Expr::Cond(cond) => {
            collect_implicit_assignment_expr_names(ctx, &cond.test, names);
            collect_implicit_assignment_expr_names(ctx, &cond.cons, names);
            collect_implicit_assignment_expr_names(ctx, &cond.alt, names);
        }
        ast::Expr::Call(call) => {
            if let ast::Callee::Expr(callee) = &call.callee {
                collect_implicit_assignment_expr_names(ctx, callee, names);
            }
            for arg in &call.args {
                collect_implicit_assignment_expr_names(ctx, &arg.expr, names);
            }
        }
        ast::Expr::New(new_expr) => {
            collect_implicit_assignment_expr_names(ctx, &new_expr.callee, names);
            if let Some(args) = &new_expr.args {
                for arg in args {
                    collect_implicit_assignment_expr_names(ctx, &arg.expr, names);
                }
            }
        }
        ast::Expr::Member(member) => {
            collect_implicit_assignment_expr_names(ctx, &member.obj, names);
            if let ast::MemberProp::Computed(computed) = &member.prop {
                collect_implicit_assignment_expr_names(ctx, &computed.expr, names);
            }
        }
        ast::Expr::Array(array) => {
            for elem in array.elems.iter().flatten() {
                collect_implicit_assignment_expr_names(ctx, &elem.expr, names);
            }
        }
        ast::Expr::Object(object) => {
            for prop in &object.props {
                match prop {
                    ast::PropOrSpread::Spread(spread) => {
                        collect_implicit_assignment_expr_names(ctx, &spread.expr, names);
                    }
                    ast::PropOrSpread::Prop(prop) => match prop.as_ref() {
                        ast::Prop::KeyValue(kv) => {
                            collect_implicit_assignment_expr_names(ctx, &kv.value, names);
                        }
                        ast::Prop::Assign(assign) => {
                            collect_implicit_assignment_expr_names(ctx, &assign.value, names);
                        }
                        ast::Prop::Method(_) => {}
                        _ => {}
                    },
                }
            }
        }
        ast::Expr::Await(await_expr) => {
            collect_implicit_assignment_expr_names(ctx, &await_expr.arg, names);
        }
        _ => {}
    }
}

fn implicit_assignment_pat_contains_name(pat: &ast::Pat, name: &str) -> bool {
    match pat {
        ast::Pat::Ident(ident) => ident.id.sym.as_ref() == name,
        ast::Pat::Array(arr) => arr
            .elems
            .iter()
            .flatten()
            .any(|elem| implicit_assignment_pat_contains_name(elem, name)),
        ast::Pat::Object(obj) => obj.props.iter().any(|prop| match prop {
            ast::ObjectPatProp::Assign(assign) => assign.key.sym.as_ref() == name,
            ast::ObjectPatProp::KeyValue(kv) => {
                implicit_assignment_pat_contains_name(&kv.value, name)
            }
            ast::ObjectPatProp::Rest(rest) => {
                implicit_assignment_pat_contains_name(&rest.arg, name)
            }
        }),
        ast::Pat::Assign(assign) => implicit_assignment_pat_contains_name(&assign.left, name),
        ast::Pat::Rest(rest) => implicit_assignment_pat_contains_name(&rest.arg, name),
        ast::Pat::Expr(_) | ast::Pat::Invalid(_) => false,
    }
}

fn implicit_assignment_target_contains_name(target: &ast::AssignTarget, name: &str) -> bool {
    match target {
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(ident)) => {
            ident.id.sym.as_ref() == name
        }
        ast::AssignTarget::Pat(pat) => match pat {
            ast::AssignTargetPat::Array(arr) => arr
                .elems
                .iter()
                .flatten()
                .any(|elem| implicit_assignment_pat_contains_name(elem, name)),
            ast::AssignTargetPat::Object(obj) => obj.props.iter().any(|prop| match prop {
                ast::ObjectPatProp::Assign(assign) => assign.key.sym.as_ref() == name,
                ast::ObjectPatProp::KeyValue(kv) => {
                    implicit_assignment_pat_contains_name(&kv.value, name)
                }
                ast::ObjectPatProp::Rest(rest) => {
                    implicit_assignment_pat_contains_name(&rest.arg, name)
                }
            }),
            ast::AssignTargetPat::Invalid(_) => false,
        },
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Paren(paren)) => {
            expr_assigns_name(&paren.expr, name)
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsAs(ts_as)) => {
            expr_assigns_name(&ts_as.expr, name)
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsNonNull(ts_nn)) => {
            expr_assigns_name(&ts_nn.expr, name)
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsTypeAssertion(ts_ta)) => {
            expr_assigns_name(&ts_ta.expr, name)
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsSatisfies(ts_sat)) => {
            expr_assigns_name(&ts_sat.expr, name)
        }
        ast::AssignTarget::Simple(
            ast::SimpleAssignTarget::Member(_)
            | ast::SimpleAssignTarget::SuperProp(_)
            | ast::SimpleAssignTarget::OptChain(_)
            | ast::SimpleAssignTarget::TsInstantiation(_)
            | ast::SimpleAssignTarget::Invalid(_),
        ) => false,
    }
}

fn expr_assigns_name(expr: &ast::Expr, name: &str) -> bool {
    match expr {
        ast::Expr::Assign(assign) => {
            implicit_assignment_target_contains_name(&assign.left, name)
                || expr_assigns_name(&assign.right, name)
        }
        ast::Expr::Seq(seq) => seq.exprs.iter().any(|expr| expr_assigns_name(expr, name)),
        ast::Expr::Paren(paren) => expr_assigns_name(&paren.expr, name),
        ast::Expr::TsAs(ts_as) => expr_assigns_name(&ts_as.expr, name),
        ast::Expr::TsNonNull(ts_nn) => expr_assigns_name(&ts_nn.expr, name),
        ast::Expr::TsTypeAssertion(ts_ta) => expr_assigns_name(&ts_ta.expr, name),
        ast::Expr::TsSatisfies(ts_sat) => expr_assigns_name(&ts_sat.expr, name),
        _ => false,
    }
}

fn assignment_target_reads_name_before_assignment(
    target: &ast::AssignTarget,
    name: &str,
    assigned: &mut bool,
) -> bool {
    match target {
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Member(member)) => {
            expr_reads_name_before_assignment(&member.obj, name, assigned)
                || matches!(
                    &member.prop,
                    ast::MemberProp::Computed(computed)
                        if expr_reads_name_before_assignment(&computed.expr, name, assigned)
                )
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::OptChain(opt_chain)) => {
            match opt_chain.base.as_ref() {
                ast::OptChainBase::Member(member) => {
                    expr_reads_name_before_assignment(&member.obj, name, assigned)
                        || matches!(
                            &member.prop,
                            ast::MemberProp::Computed(computed)
                                if expr_reads_name_before_assignment(&computed.expr, name, assigned)
                        )
                }
                ast::OptChainBase::Call(call) => {
                    if expr_reads_name_before_assignment(&call.callee, name, assigned) {
                        return true;
                    }
                    call.args
                        .iter()
                        .any(|arg| expr_reads_name_before_assignment(&arg.expr, name, assigned))
                }
            }
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Paren(paren)) => {
            expr_reads_name_before_assignment(&paren.expr, name, assigned)
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsAs(ts_as)) => {
            expr_reads_name_before_assignment(&ts_as.expr, name, assigned)
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsNonNull(ts_nn)) => {
            expr_reads_name_before_assignment(&ts_nn.expr, name, assigned)
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsTypeAssertion(ts_ta)) => {
            expr_reads_name_before_assignment(&ts_ta.expr, name, assigned)
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsSatisfies(ts_sat)) => {
            expr_reads_name_before_assignment(&ts_sat.expr, name, assigned)
        }
        ast::AssignTarget::Simple(
            ast::SimpleAssignTarget::Ident(_)
            | ast::SimpleAssignTarget::SuperProp(_)
            | ast::SimpleAssignTarget::TsInstantiation(_)
            | ast::SimpleAssignTarget::Invalid(_),
        )
        | ast::AssignTarget::Pat(_) => false,
    }
}

fn expr_reads_name_before_assignment(expr: &ast::Expr, name: &str, assigned: &mut bool) -> bool {
    match expr {
        ast::Expr::Ident(ident) => ident.sym.as_ref() == name && !*assigned,
        ast::Expr::Assign(assign) => {
            if assignment_target_reads_name_before_assignment(&assign.left, name, assigned) {
                return true;
            }
            if assign.op != ast::AssignOp::Assign
                && implicit_assignment_target_contains_name(&assign.left, name)
                && !*assigned
            {
                return true;
            }
            if expr_reads_name_before_assignment(&assign.right, name, assigned) {
                return true;
            }
            if assign.op == ast::AssignOp::Assign
                && implicit_assignment_target_contains_name(&assign.left, name)
            {
                *assigned = true;
            }
            false
        }
        ast::Expr::Seq(seq) => seq
            .exprs
            .iter()
            .any(|expr| expr_reads_name_before_assignment(expr, name, assigned)),
        ast::Expr::Paren(paren) => expr_reads_name_before_assignment(&paren.expr, name, assigned),
        ast::Expr::TsAs(ts_as) => expr_reads_name_before_assignment(&ts_as.expr, name, assigned),
        ast::Expr::TsNonNull(ts_nn) => {
            expr_reads_name_before_assignment(&ts_nn.expr, name, assigned)
        }
        ast::Expr::TsTypeAssertion(ts_ta) => {
            expr_reads_name_before_assignment(&ts_ta.expr, name, assigned)
        }
        ast::Expr::TsSatisfies(ts_sat) => {
            expr_reads_name_before_assignment(&ts_sat.expr, name, assigned)
        }
        ast::Expr::Bin(bin) => {
            expr_reads_name_before_assignment(&bin.left, name, assigned)
                || expr_reads_name_before_assignment(&bin.right, name, assigned)
        }
        ast::Expr::Unary(unary) => expr_reads_name_before_assignment(&unary.arg, name, assigned),
        ast::Expr::Update(update) => {
            if let ast::Expr::Ident(ident) = update.arg.as_ref() {
                ident.sym.as_ref() == name && !*assigned
            } else {
                expr_reads_name_before_assignment(&update.arg, name, assigned)
            }
        }
        ast::Expr::Cond(cond) => {
            if expr_reads_name_before_assignment(&cond.test, name, assigned) {
                return true;
            }
            let mut cons_assigned = *assigned;
            let mut alt_assigned = *assigned;
            let cons_reads =
                expr_reads_name_before_assignment(&cond.cons, name, &mut cons_assigned);
            let alt_reads = expr_reads_name_before_assignment(&cond.alt, name, &mut alt_assigned);
            *assigned = cons_assigned && alt_assigned;
            cons_reads || alt_reads
        }
        ast::Expr::Call(call) => {
            if let ast::Callee::Expr(callee) = &call.callee {
                if expr_reads_name_before_assignment(callee, name, assigned) {
                    return true;
                }
            }
            call.args
                .iter()
                .any(|arg| expr_reads_name_before_assignment(&arg.expr, name, assigned))
        }
        ast::Expr::New(new_expr) => {
            if expr_reads_name_before_assignment(&new_expr.callee, name, assigned) {
                return true;
            }
            new_expr.args.as_ref().is_some_and(|args| {
                args.iter()
                    .any(|arg| expr_reads_name_before_assignment(&arg.expr, name, assigned))
            })
        }
        ast::Expr::Member(member) => {
            expr_reads_name_before_assignment(&member.obj, name, assigned)
                || matches!(
                    &member.prop,
                    ast::MemberProp::Computed(computed)
                        if expr_reads_name_before_assignment(&computed.expr, name, assigned)
                )
        }
        ast::Expr::Array(array) => array
            .elems
            .iter()
            .flatten()
            .any(|elem| expr_reads_name_before_assignment(&elem.expr, name, assigned)),
        ast::Expr::Object(object) => object.props.iter().any(|prop| match prop {
            ast::PropOrSpread::Spread(spread) => {
                expr_reads_name_before_assignment(&spread.expr, name, assigned)
            }
            ast::PropOrSpread::Prop(prop) => match prop.as_ref() {
                ast::Prop::KeyValue(kv) => {
                    expr_reads_name_before_assignment(&kv.value, name, assigned)
                }
                ast::Prop::Assign(assign) => {
                    expr_reads_name_before_assignment(&assign.value, name, assigned)
                }
                _ => false,
            },
        }),
        ast::Expr::Await(await_expr) => {
            expr_reads_name_before_assignment(&await_expr.arg, name, assigned)
        }
        _ => false,
    }
}

/// Sloppy-mode simple assignment to an unresolvable reference creates a global
/// binding. Perry models the binding as a mutable local in the current lowering
/// context; this helper emits backing `Stmt::Let`s before the containing
/// statement/init so later bare reads like `x` have real storage.
pub(crate) fn predeclare_implicit_assignment_targets(
    ctx: &mut LoweringContext,
    expr: &ast::Expr,
) -> Vec<Stmt> {
    if ctx.current_strict {
        return Vec::new();
    }

    if direct_self_read_assignment_name(expr).is_some() {
        return Vec::new();
    }

    let mut names = Vec::new();
    collect_implicit_assignment_expr_names(ctx, expr, &mut names);

    let mut stmts = Vec::new();
    for name in names {
        // Inside a `with` body, an assignment to an otherwise-undeclared name
        // must route through the with object's environment record (lowered to
        // `WithSet`), not be hoisted as an implicit local that would shadow the
        // with env and capture the write locally. `WithSet` itself falls back
        // to the outer scope when the object lacks the property, so skipping
        // predeclaration here is safe for the not-on-object case too.
        if !ctx.active_with_envs_for_ident(&name).is_empty() {
            continue;
        }
        // Inside a closure (scope_depth > 0), an assignment to a name that
        // resolves to a MODULE-LEVEL binding (e.g. a `var` declared later at
        // module scope) must NOT be re-declared as a closure-local: doing so
        // emits a localizing `Let <id> = undefined` that makes codegen treat
        // the id as a closure-local instead of the module var's global slot, so
        // the write never reaches the module binding the post-declaration read
        // uses (`var f = function(){ later = 5 }; var later; f();` → undefined).
        // The assignment resolves to the module var via the global-slot path; no
        // hoist Let is needed. A closure-local shadow (`function(){ var x; … }`)
        // has its own non-module id and is unaffected.
        if ctx.scope_depth > 0 {
            if let Some(id) = ctx.lookup_local(&name) {
                if ctx.module_level_ids.contains(&id) {
                    continue;
                }
            }
        }
        let mut assigned = false;
        if !ctx.pre_registered_module_var_decls.contains(&name)
            && expr_reads_name_before_assignment(expr, &name, &mut assigned)
        {
            continue;
        }
        if ctx.lookup_class(&name).is_some() || ctx.lookup_func(&name).is_some() {
            continue;
        }
        let id = if ctx.pre_registered_module_var_decls.remove(&name) {
            ctx.pre_registered_module_vars.remove(&name);
            let id = ctx.lookup_local(&name).unwrap();
            ctx.var_hoisted_ids.insert(id);
            id
        } else if let Some(id) = ctx.lookup_local(&name) {
            id
        } else if ctx.scope_depth > 0 {
            ctx.define_sloppy_implicit_global(name.clone())
        } else {
            ctx.define_local(name.clone(), Type::Any)
        };
        if ctx.scope_depth == 0 || !ctx.sloppy_implicit_global_ids.contains(&id) {
            stmts.push(Stmt::Let {
                id,
                name,
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Undefined),
            });
        }
    }
    stmts
}

/// Detect fastify route-handler calls (`app.get|post|put|delete|patch|head|
/// options|all|addHook(path, handler)` and `app.setErrorHandler(handler)`)
/// and return the names of the first two arrow-function params — which
/// should be registered as fastify `Request` and `Reply` native instances
/// so that `request.header(...)`, `request.headers[...]`, `reply.send(...)`
/// etc. inside the handler body dispatch through the fastify FFI instead
/// of falling through to generic object access.
///
/// Returns `Some((request_name, reply_name))` when the pattern matches,
/// where `reply_name` is empty if the handler takes only one param.
/// Returns `None` for any other call shape.
pub(crate) fn pre_scan_fastify_handler_params(
    ctx: &crate::lower::LoweringContext,
    call: &ast::CallExpr,
) -> Option<(String, String)> {
    use ast::Callee;
    let callee_expr = match &call.callee {
        Callee::Expr(e) => e,
        _ => return None,
    };
    // Callee must be `<obj>.<method>` where <obj> is a registered fastify
    // App (or a chain that resolves to one; for simplicity we only handle
    // the direct Ident case here — app.get(...), not getApp().get(...)).
    let member = match callee_expr.as_ref() {
        ast::Expr::Member(m) => m,
        _ => return None,
    };
    let obj_ident = match member.obj.as_ref() {
        ast::Expr::Ident(i) => i,
        _ => return None,
    };
    let obj_name = obj_ident.sym.to_string();
    let native = ctx.lookup_native_instance(&obj_name)?;
    if native.0 != "fastify" {
        return None;
    }
    let method_name = match &member.prop {
        ast::MemberProp::Ident(i) => i.sym.to_string(),
        _ => return None,
    };
    // The handler is the last arg for route methods (skip the path arg).
    // - `app.get(path, handler)`    → handler_idx = 1
    // - `app.setErrorHandler(hnd)`  → handler_idx = 0
    // - `app.addHook(name, hnd)`    → handler_idx = 1
    let handler_idx = match method_name.as_str() {
        "get" | "post" | "put" | "delete" | "patch" | "head" | "options" | "all" => 1,
        "addHook" => 1,
        "setErrorHandler" => 0,
        _ => return None,
    };
    let handler_arg = call.args.get(handler_idx)?;
    if handler_arg.spread.is_some() {
        return None;
    }
    let arrow = match handler_arg.expr.as_ref() {
        ast::Expr::Arrow(a) => a,
        ast::Expr::Fn(_) => return None, // fn expressions handled separately
        _ => return None,
    };
    // Issue #1070: `setErrorHandler(async (err, req, reply) => …)` —
    // the first arrow param is the THROWN VALUE, not a fastify Request.
    // Registering `err` as `("fastify", "Request")` causes `err.problem`
    // (or any user-field access on a thrown class instance) to lower to
    // a NativeMethodCall whose method name isn't in the fastify Request
    // dispatch table → the lower_native_method_call fall-through emits
    // `double_literal(0.0)`, so the access prints as `0`. Skip the first
    // arrow param for setErrorHandler so only params[1] / params[2]
    // (the real Request / Reply) get the native-instance tags.
    let (req_param_idx, reply_param_idx) = if method_name == "setErrorHandler" {
        (1, 2)
    } else {
        (0, 1)
    };
    let req_name = arrow.params.get(req_param_idx).and_then(pat_ident_name)?;
    let reply_name = arrow
        .params
        .get(reply_param_idx)
        .and_then(pat_ident_name)
        .unwrap_or_default();
    Some((req_name, reply_name))
}

/// Extract a plain-ident name from an arrow function param (skip
/// destructured / rest params — those aren't Request/Reply by shape).
fn pat_ident_name(pat: &ast::Pat) -> Option<String> {
    match pat {
        ast::Pat::Ident(i) => Some(i.id.sym.to_string()),
        _ => None,
    }
}

/// Pre-scan for `http.createServer((req, res) => ...)` and
/// `createServer((req, res) => ...)` (named import from node:http).
/// Issue #577 mirror of `pre_scan_fastify_handler_params`. Returns
/// the (request_local, response_local) names so the caller can
/// register them as `("http", "IncomingMessage")` and
/// `("http", "ServerResponse")` native instances BEFORE the arrow
/// body is lowered — that way `req.method` / `res.end(...)` inside
/// the handler dispatch through NATIVE_MODULE_TABLE.
///
/// Returns `None` for any other call shape.
pub(crate) fn pre_scan_node_http_create_server_params(
    ctx: &crate::lower::LoweringContext,
    call: &ast::CallExpr,
) -> Option<(String, String)> {
    use ast::Callee;
    let callee_expr = match &call.callee {
        Callee::Expr(e) => e,
        _ => return None,
    };

    let (module_name, method_name) = match callee_expr.as_ref() {
        ast::Expr::Member(member) => {
            let obj_ident = match member.obj.as_ref() {
                ast::Expr::Ident(i) => i,
                _ => return None,
            };
            let obj_name = obj_ident.sym.to_string();
            let (module, _) = ctx.lookup_native_module(&obj_name)?;
            let method = match &member.prop {
                ast::MemberProp::Ident(i) => i.sym.to_string(),
                _ => return None,
            };
            (module.to_string(), method)
        }
        ast::Expr::Ident(ident) => {
            let func_name = ident.sym.to_string();
            let (module, method_opt) = ctx.lookup_native_module(&func_name)?;
            let method = method_opt?.to_string();
            (module.to_string(), method)
        }
        _ => return None,
    };

    let _matched = match (module_name.as_str(), method_name.as_str()) {
        ("http", "createServer") => true,
        ("https", "createServer") => true,
        ("http2", "createSecureServer") => true,
        _ => return None,
    };

    let handler_arg = call.args.last()?;
    if handler_arg.spread.is_some() {
        return None;
    }
    let arrow = match handler_arg.expr.as_ref() {
        ast::Expr::Arrow(a) => a,
        _ => return None,
    };
    let req_name = arrow.params.first().and_then(pat_ident_name)?;
    let res_name = arrow
        .params
        .get(1)
        .and_then(pat_ident_name)
        .unwrap_or_default();
    if res_name.is_empty() {
        return None;
    }
    Some((req_name, res_name))
}

/// Pre-scan for `http.get(url, (res) => …)` / `http.request(opts, (res) =>
/// …)` / `https.get` / `https.request`. Issue #1124 followup — the
/// `data` / `end` listeners on the IncomingMessage that arrives at the
/// response callback need to dispatch via NATIVE_MODULE_TABLE entries
/// (class_filter = Some("IncomingMessage")). Pre-fix the `(res)` param
/// was untagged so `res.on('data', cb)` fell through to
/// `js_native_call_method` → small-handle dispatch → no IncomingMessage
/// `on` arm → listener never registered → `'end'` never fired and the
/// (post-#1124-followup) Buffer body never flowed to the user.
///
/// Mirrors `pre_scan_node_http_create_server_params` shape but for the
/// CLIENT factory + single-param `(res)` arrow shape.
///
/// Returns `Some(res_local_name)` when the pattern matches.
pub(crate) fn pre_scan_node_http_client_callback_params(
    ctx: &crate::lower::LoweringContext,
    call: &ast::CallExpr,
) -> Option<String> {
    use ast::Callee;
    let callee_expr = match &call.callee {
        Callee::Expr(e) => e,
        _ => return None,
    };

    let (module_name, method_name) = match callee_expr.as_ref() {
        ast::Expr::Member(member) => {
            let obj_ident = match member.obj.as_ref() {
                ast::Expr::Ident(i) => i,
                _ => return None,
            };
            let obj_name = obj_ident.sym.to_string();
            let (module, _) = ctx.lookup_native_module(&obj_name)?;
            let method = match &member.prop {
                ast::MemberProp::Ident(i) => i.sym.to_string(),
                _ => return None,
            };
            (module.to_string(), method)
        }
        ast::Expr::Ident(ident) => {
            let func_name = ident.sym.to_string();
            let (module, method_opt) = ctx.lookup_native_module(&func_name)?;
            let method = method_opt?.to_string();
            (module.to_string(), method)
        }
        _ => return None,
    };

    // Only http/https request/get factories. http2's `connect()` returns a
    // ClientHttp2Session — different surface, separate pre-scan.
    let _matched = match (module_name.as_str(), method_name.as_str()) {
        ("http", "get" | "request") => true,
        ("https", "get" | "request") => true,
        _ => return None,
    };

    // The response callback is the last arrow/function arg. Walk
    // backwards so options-then-cb shapes (`http.request(opts, cb)`)
    // and url-then-cb shapes (`http.get(url, cb)`) both resolve.
    let handler_arg = call.args.last()?;
    if handler_arg.spread.is_some() {
        return None;
    }
    let arrow = match handler_arg.expr.as_ref() {
        ast::Expr::Arrow(a) => a,
        _ => return None,
    };
    // First (and typically only) arrow param is the IncomingMessage.
    arrow.params.first().and_then(pat_ident_name)
}

/// Pre-scan for `httpServer.on('upgrade', (req, wsId, head) => …)`
/// (issue #577 Phase 4). When the receiver is a registered HttpServer
/// native instance and the event name is `'upgrade'`, register the
/// SECOND arrow param (`wsId`) as a `("ws", "Client")` native instance
/// BEFORE the body is lowered, so calls inside the handler like
/// `wsId.send(...)` / `wsId.on('message', cb)` / `wsId.close()`
/// dispatch through the dedicated Client-class entries in
/// NATIVE_MODULE_TABLE (which call the `js_ws_send_client_i64` /
/// `js_ws_close_client_i64` / `js_ws_on_client_i64` shims that take
/// the receiver as `i64` after `unbox_to_i64` — the wsId arrives
/// from the upgrade dispatch NaN-boxed POINTER_TAG so the unbox
/// extracts the raw integer correctly).
///
/// Returns `Some(wsId_local_name)` when the pattern matches.
pub(crate) fn pre_scan_node_http_upgrade_params(
    ctx: &crate::lower::LoweringContext,
    call: &ast::CallExpr,
) -> Option<String> {
    use ast::Callee;
    let callee_expr = match &call.callee {
        Callee::Expr(e) => e,
        _ => return None,
    };
    let member = match callee_expr.as_ref() {
        ast::Expr::Member(m) => m,
        _ => return None,
    };
    let obj_ident = match member.obj.as_ref() {
        ast::Expr::Ident(i) => i,
        _ => return None,
    };
    let obj_name = obj_ident.sym.to_string();
    let (module, class) = ctx.lookup_native_instance(&obj_name)?;
    if module != "http" || class != "HttpServer" {
        return None;
    }
    let method_name = match &member.prop {
        ast::MemberProp::Ident(i) => i.sym.to_string(),
        _ => return None,
    };
    if method_name != "on" && method_name != "addListener" {
        return None;
    }
    // First arg must be the literal string "upgrade".
    let event_arg = call.args.first()?;
    let event_name = match event_arg.expr.as_ref() {
        ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str().unwrap_or(""),
        _ => return None,
    };
    if event_name != "upgrade" {
        return None;
    }
    // Second arg = handler. Pull the second param (wsId).
    let handler_arg = call.args.get(1)?;
    if handler_arg.spread.is_some() {
        return None;
    }
    let arrow = match handler_arg.expr.as_ref() {
        ast::Expr::Arrow(a) => a,
        _ => return None,
    };
    let ws_id_name = arrow.params.get(1).and_then(pat_ident_name)?;
    Some(ws_id_name)
}

/// Issue #2211 — pre-scan for `request.on('socket', sock => …)` (and the
/// `'connect'`/`'connection'` aliases). When the receiver is a
/// `ClientRequest` native instance and the event name is `'socket'`,
/// register the SINGLE arrow param as a `("net", "Socket")` native
/// instance BEFORE the body is lowered, so introspection calls inside
/// the handler — `sock.listeners('timeout')`, `sock.eventNames()`,
/// `sock.removeListener(...)` — dispatch through the class-filtered
/// Socket rows in NATIVE_MODULE_TABLE instead of failing the codegen-
/// emitted `value is not a function` check.
///
/// Returns `Some(socket_local_name)` when the pattern matches.
pub(crate) fn pre_scan_node_http_client_request_socket_params(
    ctx: &crate::lower::LoweringContext,
    call: &ast::CallExpr,
) -> Option<String> {
    use ast::Callee;
    let callee_expr = match &call.callee {
        Callee::Expr(e) => e,
        _ => return None,
    };
    let member = match callee_expr.as_ref() {
        ast::Expr::Member(m) => m,
        _ => return None,
    };
    let obj_ident = match member.obj.as_ref() {
        ast::Expr::Ident(i) => i,
        _ => return None,
    };
    let obj_name = obj_ident.sym.to_string();
    let (module, class) = ctx.lookup_native_instance(&obj_name)?;
    if module != "http" || class != "ClientRequest" {
        return None;
    }
    let method_name = match &member.prop {
        ast::MemberProp::Ident(i) => i.sym.to_string(),
        _ => return None,
    };
    if method_name != "on" && method_name != "addListener" && method_name != "once" {
        return None;
    }
    // First arg must be `'socket'` / `'connect'` / `'connection'`. Node
    // fires the same socket reference on all three so the same param-tag
    // applies; pinning the literal here keeps unrelated events (`'response'`,
    // `'error'`) untouched.
    let event_arg = call.args.first()?;
    let event_name = match event_arg.expr.as_ref() {
        ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str().unwrap_or(""),
        _ => return None,
    };
    if !matches!(event_name, "socket" | "connect" | "connection") {
        return None;
    }
    let handler_arg = call.args.get(1)?;
    if handler_arg.spread.is_some() {
        return None;
    }
    let arrow = match handler_arg.expr.as_ref() {
        ast::Expr::Arrow(a) => a,
        _ => return None,
    };
    arrow.params.first().and_then(pat_ident_name)
}

/// Detect if an expression represents a native handle instance (Big, Decimal, etc.)
/// Returns the module name if it does.
///
/// A user `class Big {...}` (or `Decimal`, etc.) in the current module shadows
/// the hardcoded library-name mapping — without that gate `class Big { f0=0; }
/// const b = new Big(); b.f0` returned 0 because the value was routed through
/// big.js's handle-based dispatch.
pub(crate) fn detect_native_instance_expr(
    ctx: &LoweringContext,
    expr: &ast::Expr,
) -> Option<&'static str> {
    match expr {
        // new Big(...) / new Decimal(...) / new BigNumber(...)
        ast::Expr::New(new_expr) => {
            if let ast::Expr::Ident(ident) = new_expr.callee.as_ref() {
                let class_name = ident.sym.as_ref();
                if ctx.classes_index.contains_key(class_name)
                    || ctx.pending_classes.iter().any(|c| c.name == class_name)
                {
                    return None;
                }
                match class_name {
                    "Big" => Some("big.js"),
                    "Decimal" => Some("decimal.js"),
                    "BigNumber" => Some("bignumber.js"),
                    "LRUCache" => Some("lru-cache"),
                    "Command" => Some("commander"),
                    _ => None,
                }
            } else {
                None
            }
        }
        // Chained method calls: new Big(...).plus(...).div(...)
        ast::Expr::Call(call_expr) => {
            if let ast::Callee::Expr(callee_expr) = &call_expr.callee {
                if let ast::Expr::Member(member) = callee_expr.as_ref() {
                    // Recursively check the object
                    detect_native_instance_expr(ctx, &member.obj)
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Check if a parameter pattern is a rest parameter (...args)
pub(crate) fn is_rest_param(pat: &ast::Pat) -> bool {
    matches!(pat, ast::Pat::Rest(_))
}

/// Extract default value from a parameter pattern (if any)
/// For optional parameters (x?: Type), we provide Expr::Undefined as the default
pub(crate) fn get_param_default(ctx: &mut LoweringContext, pat: &ast::Pat) -> Result<Option<Expr>> {
    match pat {
        ast::Pat::Ident(ident) => {
            // Check if this is an optional parameter (x?: Type)
            if ident.optional {
                Ok(Some(Expr::Undefined))
            } else {
                Ok(None)
            }
        }
        ast::Pat::Assign(assign) => {
            let default_expr = lower_expr(ctx, &assign.right)?;
            Ok(Some(default_expr))
        }
        _ => Ok(None),
    }
}

// #5216: `is_require_builtin_module` (fs/path/crypto-only) and its
// `BUILTIN_MODULES` table were removed — `require("<spec>")` of a resolvable
// native/Node-builtin module is now handled generically by
// `destructuring::var_decl_sources::require_resolvable_native_specifier`, which
// subsumes the old narrow allowlist.
