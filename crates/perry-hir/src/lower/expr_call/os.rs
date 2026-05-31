use swc_ecma_ast as ast;

use crate::ir::Expr;

/// Lower an `os.userInfo(...)` call. `args` are the already-lowered call
/// arguments (consumed for the dynamic path).
///
/// Three cases (#3004):
/// - statically-visible object literal `{ encoding: "buffer" }` → the
///   `OsUserInfoBuffer` fast path (no runtime options inspection);
/// - no arguments, or a statically-visible options literal that does NOT
///   request the buffer encoding → the plain `OsUserInfo` string path;
/// - anything dynamic (a variable, function return, computed-key object, or
///   spread) → a generic `NativeMethodCall` that carries the argument through
///   to runtime dispatch, where `options.encoding` is inspected at runtime.
pub(super) fn user_info_expr_for_call(call: &ast::CallExpr, args: Vec<Expr>) -> Expr {
    match static_encoding_decision(call) {
        Some(true) => Expr::OsUserInfoBuffer,
        Some(false) => Expr::OsUserInfo,
        None => Expr::NativeMethodCall {
            module: "os".to_string(),
            class_name: None,
            object: None,
            method: "userInfo".to_string(),
            args,
        },
    }
}

/// Decide the encoding statically when possible.
/// - `Some(true)`  — literal `{ encoding: "buffer" }` (with a non-computed key).
/// - `Some(false)` — no arguments, or a fully-literal options object whose
///   `encoding` is statically known not to be `"buffer"`.
/// - `None`        — the options value is dynamic; defer to runtime dispatch.
fn static_encoding_decision(call: &ast::CallExpr) -> Option<bool> {
    let Some(first) = call.args.first() else {
        // No options argument → always strings.
        return Some(false);
    };
    if first.spread.is_some() {
        return None;
    }
    let ast::Expr::Object(obj) = unwrap_ts_wrappers(first.expr.as_ref()) else {
        // Variable / call result / non-literal → inspect at runtime.
        return None;
    };

    // A spread inside the literal, or a computed/non-literal `encoding` value,
    // can change the effective encoding at runtime → defer.
    for prop in &obj.props {
        match prop {
            ast::PropOrSpread::Spread(_) => return None,
            ast::PropOrSpread::Prop(prop) => {
                let ast::Prop::KeyValue(kv) = prop.as_ref() else {
                    continue;
                };
                // A computed key (e.g. `{ ["encoding"]: "buffer" }`) is not
                // statically resolvable here — defer to runtime so the actual
                // `options.encoding` is inspected.
                if matches!(kv.key, ast::PropName::Computed(_)) {
                    return None;
                }
                if !prop_name_is(&kv.key, "encoding") {
                    continue;
                }
                // Non-string-literal value → cannot decide statically.
                return match unwrap_ts_wrappers(kv.value.as_ref()) {
                    ast::Expr::Lit(ast::Lit::Str(s)) => Some(s.value.as_str() == Some("buffer")),
                    _ => None,
                };
            }
        }
    }

    // Literal object with no `encoding` key → strings.
    Some(false)
}

fn prop_name_is(name: &ast::PropName, expected: &str) -> bool {
    match name {
        ast::PropName::Ident(ident) => ident.sym == expected,
        ast::PropName::Str(s) => s.value.as_str() == Some(expected),
        _ => false,
    }
}

fn unwrap_ts_wrappers(e: &ast::Expr) -> &ast::Expr {
    let mut cur = e;
    loop {
        match cur {
            ast::Expr::TsAs(x) => cur = &x.expr,
            ast::Expr::TsNonNull(x) => cur = &x.expr,
            ast::Expr::TsSatisfies(x) => cur = &x.expr,
            ast::Expr::TsTypeAssertion(x) => cur = &x.expr,
            ast::Expr::TsConstAssertion(x) => cur = &x.expr,
            ast::Expr::Paren(x) => cur = x.expr.as_ref(),
            _ => return cur,
        }
    }
}
