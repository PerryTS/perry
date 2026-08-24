use swc_ecma_ast as ast;

use super::super::fn_ctor_env::DynFnCtorKind;

/// CreateDynamicFunction's parameter early errors are kind-sensitive. Inspect
/// the parsed parameter AST so keyword-looking text in comments or string
/// literals is ignored while real `yield` / `await` syntax is rejected.
pub(super) fn fn_ctor_kind_param_early_error(fn_expr: &ast::FnExpr, kind: DynFnCtorKind) -> bool {
    let forbidden = match kind {
        DynFnCtorKind::Generator => &["yield"][..],
        DynFnCtorKind::Async => &["await"][..],
        DynFnCtorKind::AsyncGenerator => &["yield", "await"][..],
        DynFnCtorKind::Plain => return false,
    };

    fn prop_name_has(name: &ast::PropName, forbidden: &[&str]) -> bool {
        matches!(name, ast::PropName::Computed(c) if expr_has(&c.expr, forbidden))
    }

    fn pat_has(pat: &ast::Pat, forbidden: &[&str]) -> bool {
        match pat {
            ast::Pat::Ident(id) => forbidden.contains(&id.id.sym.as_ref()),
            ast::Pat::Array(array) => array
                .elems
                .iter()
                .flatten()
                .any(|pat| pat_has(pat, forbidden)),
            ast::Pat::Object(object) => object.props.iter().any(|prop| match prop {
                ast::ObjectPatProp::KeyValue(kv) => {
                    prop_name_has(&kv.key, forbidden) || pat_has(&kv.value, forbidden)
                }
                ast::ObjectPatProp::Assign(assign) => {
                    forbidden.contains(&assign.key.sym.as_ref())
                        || assign
                            .value
                            .as_deref()
                            .is_some_and(|value| expr_has(value, forbidden))
                }
                ast::ObjectPatProp::Rest(rest) => pat_has(&rest.arg, forbidden),
            }),
            ast::Pat::Assign(assign) => {
                pat_has(&assign.left, forbidden) || expr_has(&assign.right, forbidden)
            }
            ast::Pat::Rest(rest) => pat_has(&rest.arg, forbidden),
            ast::Pat::Expr(expr) => expr_has(expr, forbidden),
            ast::Pat::Invalid(_) => false,
        }
    }

    fn expr_has(expr: &ast::Expr, forbidden: &[&str]) -> bool {
        match expr {
            ast::Expr::Ident(id) => forbidden.contains(&id.sym.as_ref()),
            ast::Expr::Yield(_) => forbidden.contains(&"yield"),
            ast::Expr::Await(await_expr) => {
                forbidden.contains(&"await") || expr_has(&await_expr.arg, forbidden)
            }
            ast::Expr::Paren(paren) => expr_has(&paren.expr, forbidden),
            ast::Expr::Unary(unary) => expr_has(&unary.arg, forbidden),
            ast::Expr::Update(update) => expr_has(&update.arg, forbidden),
            ast::Expr::Bin(binary) => {
                expr_has(&binary.left, forbidden) || expr_has(&binary.right, forbidden)
            }
            ast::Expr::Assign(assign) => expr_has(&assign.right, forbidden),
            ast::Expr::Cond(cond) => {
                expr_has(&cond.test, forbidden)
                    || expr_has(&cond.cons, forbidden)
                    || expr_has(&cond.alt, forbidden)
            }
            ast::Expr::Seq(seq) => seq.exprs.iter().any(|expr| expr_has(expr, forbidden)),
            ast::Expr::Member(member) => {
                expr_has(&member.obj, forbidden)
                    || matches!(&member.prop, ast::MemberProp::Computed(c) if expr_has(&c.expr, forbidden))
            }
            ast::Expr::Call(call) => {
                matches!(&call.callee, ast::Callee::Expr(expr) if expr_has(expr, forbidden))
                    || call.args.iter().any(|arg| expr_has(&arg.expr, forbidden))
            }
            ast::Expr::New(new_expr) => {
                expr_has(&new_expr.callee, forbidden)
                    || new_expr
                        .args
                        .as_ref()
                        .is_some_and(|args| args.iter().any(|arg| expr_has(&arg.expr, forbidden)))
            }
            ast::Expr::Array(array) => array
                .elems
                .iter()
                .flatten()
                .any(|elem| expr_has(&elem.expr, forbidden)),
            ast::Expr::Object(object) => object.props.iter().any(|prop| match prop {
                ast::PropOrSpread::Spread(spread) => expr_has(&spread.expr, forbidden),
                ast::PropOrSpread::Prop(prop) => match prop.as_ref() {
                    ast::Prop::KeyValue(kv) => {
                        prop_name_has(&kv.key, forbidden) || expr_has(&kv.value, forbidden)
                    }
                    ast::Prop::Assign(assign) => expr_has(&assign.value, forbidden),
                    ast::Prop::Getter(getter) => prop_name_has(&getter.key, forbidden),
                    ast::Prop::Setter(setter) => prop_name_has(&setter.key, forbidden),
                    ast::Prop::Method(method) => prop_name_has(&method.key, forbidden),
                    ast::Prop::Shorthand(id) => forbidden.contains(&id.sym.as_ref()),
                },
            }),
            ast::Expr::TsAs(ts) => expr_has(&ts.expr, forbidden),
            ast::Expr::TsTypeAssertion(ts) => expr_has(&ts.expr, forbidden),
            ast::Expr::TsConstAssertion(ts) => expr_has(&ts.expr, forbidden),
            ast::Expr::TsNonNull(ts) => expr_has(&ts.expr, forbidden),
            _ => false,
        }
    }

    fn_expr
        .function
        .params
        .iter()
        .any(|param| pat_has(&param.pat, forbidden))
}
