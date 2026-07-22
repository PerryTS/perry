//! Regex .test()/.exec() and String .match(regex) on arbitrary receivers.
//!
//! Extracted from `expr_call/mod.rs` as a mechanical move.

use crate::types::Type;
use anyhow::Result;
use swc_ecma_ast as ast;

use crate::ir::*;

use super::super::{lower_expr, LoweringContext};

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
                    // Check if the object is a regex literal or a local assigned to a regex.
                    //
                    // CRITICAL (#A — semver/minimatch source-compile): do NOT
                    // treat `x.test(arg)` as a RegExp test merely because `x` is
                    // an `Any`/`Unknown`/untyped local. `.test()` is also a
                    // common INSTANCE method name (semver's `Comparator.test` /
                    // `Range.test`, etc.). An imported class instance is typed
                    // `Any` at the call site, so the old `Any | Unknown |
                    // unwrap_or(true)` heuristic mis-lowered `comparator.test(v)`
                    // into `js_regexp_test(comparator-coerced-to-string, ...)`,
                    // silently returning a bogus boolean and never running the
                    // real method body — breaking `semver.satisfies` /
                    // `minimatch(...)`. The runtime already routes a genuine
                    // RegExp receiver's `.test()`/`.exec()` through the dynamic
                    // method-dispatch path (`dispatch_regex_receiver_method` in
                    // `js_native_call_method`, #1731), so falling through to a
                    // normal method call is correct for BOTH a regex value
                    // (runtime regex dispatch) and a class instance (instance
                    // method). Only take the codegen fast path when we have
                    // positive evidence the receiver is a regex.
                    let is_regex_obj = match member.obj.as_ref() {
                        ast::Expr::Lit(ast::Lit::Regex(_)) => true,
                        ast::Expr::Ident(ident) => ctx
                            .lookup_local_type(ident.sym.as_ref())
                            .map(|ty| matches!(ty, Type::Named(n) if n == "RegExp"))
                            .unwrap_or(false),
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
                    // Only fold to `String.prototype.match`/`matchAll` when the
                    // RECEIVER is statically a string. `.match` is also a common
                    // user-class method name (Next.js route matchers'
                    // `RouteMatcher.match(pathname)`), so an unknown / `Any` /
                    // class-instance receiver must NOT be assumed a string —
                    // otherwise `m.match(p)` on a class instance compiled to
                    // `js_string_match(m_as_string, p)`, reinterpreting the
                    // instance pointer as a string and returning null (Next.js
                    // wall 52: `DefaultRouteMatcherManager.validate` →
                    // `matcher.match(pathname)` never matched the App-Router root
                    // "/" → HTTP 500). A receiver that really is a string still
                    // gets `match` two ways: this fold (statically-typed string),
                    // and the `jsval.is_string()` arm of `js_native_call_method`
                    // for `Any`-typed strings. Mirrors the wall-50 `normalize`
                    // fix.
                    let recv_is_string = match member.obj.as_ref() {
                        ast::Expr::Lit(ast::Lit::Str(_)) => true,
                        ast::Expr::Tpl(_) => true,
                        ast::Expr::Ident(ident) => {
                            matches!(
                                ctx.lookup_local_type(ident.sym.as_ref()),
                                Some(Type::String)
                            )
                        }
                        _ => false,
                    };
                    if !recv_is_string {
                        return Ok(Err(args));
                    }
                    // Check if the argument is a regex literal or a local holding a regex
                    let arg_is_regex = match call.args.first().map(|a| a.expr.as_ref()) {
                        Some(ast::Expr::Lit(ast::Lit::Regex(_))) => true,
                        Some(ast::Expr::Ident(ident)) => {
                            matches!(
                                ctx.lookup_local_type(ident.sym.as_ref()),
                                Some(Type::Named(n)) if n == "RegExp"
                            )
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
