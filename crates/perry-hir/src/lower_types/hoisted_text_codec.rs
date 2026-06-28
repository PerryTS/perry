use perry_types::Type;
use swc_ecma_ast as ast;

use super::extract_ts_type;

pub(crate) fn peel_expr_for_hoisted_var_type(expr: &ast::Expr) -> &ast::Expr {
    match expr {
        ast::Expr::Paren(paren) => peel_expr_for_hoisted_var_type(&paren.expr),
        ast::Expr::TsAs(ts_as) => peel_expr_for_hoisted_var_type(&ts_as.expr),
        ast::Expr::TsTypeAssertion(ts_assert) => peel_expr_for_hoisted_var_type(&ts_assert.expr),
        ast::Expr::TsNonNull(non_null) => peel_expr_for_hoisted_var_type(&non_null.expr),
        ast::Expr::TsConstAssertion(const_assert) => {
            peel_expr_for_hoisted_var_type(&const_assert.expr)
        }
        _ => expr,
    }
}

pub(crate) fn require_literal_specifier(expr: &ast::Expr) -> Option<&str> {
    let ast::Expr::Call(call) = peel_expr_for_hoisted_var_type(expr) else {
        return None;
    };
    let ast::Callee::Expr(callee) = &call.callee else {
        return None;
    };
    let ast::Expr::Ident(ident) = callee.as_ref() else {
        return None;
    };
    if ident.sym.as_ref() != "require" {
        return None;
    }
    let first_arg = call.args.first()?;
    let ast::Expr::Lit(ast::Lit::Str(specifier)) = peel_expr_for_hoisted_var_type(&first_arg.expr)
    else {
        return None;
    };
    Some(specifier.value.as_str().unwrap_or(""))
}

pub(crate) fn infer_hoisted_text_codec_var_type(
    decl: &ast::VarDeclarator,
    ident: &ast::BindingIdent,
    is_util_alias: impl Fn(&str) -> bool,
) -> Type {
    if let Some(ann) = ident.type_ann.as_ref() {
        return extract_ts_type(&ann.type_ann);
    }

    let Some(init) = decl.init.as_deref().map(peel_expr_for_hoisted_var_type) else {
        return Type::Any;
    };
    let ast::Expr::New(new_expr) = init else {
        return Type::Any;
    };

    match peel_expr_for_hoisted_var_type(new_expr.callee.as_ref()) {
        ast::Expr::Ident(ctor) => match ctor.sym.as_ref() {
            "TextEncoder" | "TextDecoder" => Type::Named(ctor.sym.to_string()),
            _ => Type::Any,
        },
        ast::Expr::Member(member) => {
            let (ast::Expr::Ident(obj), ast::MemberProp::Ident(prop)) =
                (member.obj.as_ref(), &member.prop)
            else {
                return Type::Any;
            };
            let prop_name = prop.sym.as_ref();
            if matches!(prop_name, "TextEncoder" | "TextDecoder") && is_util_alias(obj.sym.as_ref())
            {
                Type::Named(prop_name.to_string())
            } else {
                Type::Any
            }
        }
        _ => Type::Any,
    }
}
