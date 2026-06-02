//! Regex .test()/.exec() and String .match(regex) on arbitrary receivers.
//!
//! Extracted from `expr_call/mod.rs` as a mechanical move.

use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower_types::extract_ts_type_with_ctx;

use super::super::{
    extract_typed_parse_source_order, is_generator_call_expr, is_widget_modifier_name, lower_expr,
    resolve_typed_parse_ty, LoweringContext,
};

pub(super) fn try_regex_string_methods(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    mut args: Vec<Expr>,
) -> Result<Result<Expr, Vec<Expr>>> {
    // Check for regex .test() / .exec() method call on any expression
    if let ast::Callee::Expr(callee_expr) = &call.callee {
        if let ast::Expr::Member(member) = callee_expr.as_ref() {
            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                let m = method_ident.sym.as_ref();
                if (m == "test" || m == "exec") && args.len() == 1 {
                    // Check if the object is a regex literal or a local assigned to a regex
                    let is_regex_obj = match member.obj.as_ref() {
                        ast::Expr::Lit(ast::Lit::Regex(_)) => true,
                        ast::Expr::Ident(ident) => ctx
                            .lookup_local_type(ident.sym.as_ref())
                            .map(|ty| {
                                matches!(ty, Type::Any | Type::Unknown)
                                    || matches!(ty, Type::Named(n) if n == "RegExp")
                            })
                            .unwrap_or(true),
                        _ => false,
                    };
                    if is_regex_obj {
                        let regex_expr = lower_expr(ctx, &member.obj)?;
                        // Only emit RegExp method calls if the object is actually a regex
                        if matches!(&regex_expr, Expr::RegExp { .. })
                            || matches!(&regex_expr, Expr::LocalGet(_))
                        {
                            let string_expr = args.into_iter().next().unwrap();
                            if m == "test" {
                                return Ok(Ok(Expr::RegExpTest {
                                    regex: Box::new(regex_expr),
                                    string: Box::new(string_expr),
                                }));
                            } else {
                                return Ok(Ok(Expr::RegExpExec {
                                    regex: Box::new(regex_expr),
                                    string: Box::new(string_expr),
                                }));
                            }
                        }
                    }
                }
            }
        }
    }

    // Check for string .match(regex) method call
    if let ast::Callee::Expr(callee_expr) = &call.callee {
        if let ast::Expr::Member(member) = callee_expr.as_ref() {
            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                if (method_ident.sym.as_ref() == "match" || method_ident.sym.as_ref() == "matchAll")
                    && args.len() == 1
                {
                    let is_match_all = method_ident.sym.as_ref() == "matchAll";
                    // Check if the argument is a regex literal or a local holding a regex
                    let arg_is_regex = match call.args.first().map(|a| a.expr.as_ref()) {
                        Some(ast::Expr::Lit(ast::Lit::Regex(_))) => true,
                        Some(ast::Expr::Ident(ident)) => {
                            match ctx.lookup_local_type(ident.sym.as_ref()) {
                                // Known regex local
                                Some(Type::Named(n)) if n == "RegExp" => true,
                                // Unknown type — assume could be regex
                                Some(Type::Any) | Some(Type::Unknown) | None => true,
                                _ => false,
                            }
                        }
                        _ => false,
                    };
                    if arg_is_regex {
                        let string_expr = lower_expr(ctx, &member.obj)?;
                        let regex_expr = args.remove(0);
                        if matches!(&regex_expr, Expr::RegExp { .. })
                            || matches!(&regex_expr, Expr::LocalGet(_))
                        {
                            return Ok(Ok(if is_match_all {
                                Expr::StringMatchAll {
                                    string: Box::new(string_expr),
                                    regex: Box::new(regex_expr),
                                }
                            } else {
                                Expr::StringMatch {
                                    string: Box::new(string_expr),
                                    regex: Box::new(regex_expr),
                                }
                            }));
                        }
                    }
                }
            }
        }
    }

    Ok(Err(args))
}
