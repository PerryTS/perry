use swc_ecma_ast as ast;

use crate::ir::Expr;

pub(super) fn user_info_expr_for_call(call: &ast::CallExpr) -> Expr {
    if first_arg_has_buffer_encoding(call) {
        Expr::OsUserInfoBuffer
    } else {
        Expr::OsUserInfo
    }
}

fn first_arg_has_buffer_encoding(call: &ast::CallExpr) -> bool {
    let Some(first) = call.args.first() else {
        return false;
    };
    if first.spread.is_some() {
        return false;
    }
    let ast::Expr::Object(obj) = unwrap_ts_wrappers(first.expr.as_ref()) else {
        return false;
    };

    obj.props.iter().any(|prop| {
        let ast::PropOrSpread::Prop(prop) = prop else {
            return false;
        };
        let ast::Prop::KeyValue(kv) = prop.as_ref() else {
            return false;
        };
        prop_name_is(&kv.key, "encoding") && string_literal_is(kv.value.as_ref(), "buffer")
    })
}

fn prop_name_is(name: &ast::PropName, expected: &str) -> bool {
    match name {
        ast::PropName::Ident(ident) => ident.sym == expected,
        ast::PropName::Str(s) => s.value.as_str() == Some(expected),
        _ => false,
    }
}

fn string_literal_is(expr: &ast::Expr, expected: &str) -> bool {
    match unwrap_ts_wrappers(expr) {
        ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str() == Some(expected),
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
