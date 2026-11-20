//! Static arguments are syntax values, never inferred TypeScript types.

use super::{binding, Invocation};
use anyhow::{bail, Context, Result};
use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitWith};

fn constant(module: &ast::Module, id: &ast::Id) -> Option<Box<ast::Expr>> {
    struct Find<'a> {
        id: &'a ast::Id,
        value: Option<Box<ast::Expr>>,
    }
    impl Visit for Find<'_> {
        fn visit_var_decl(&mut self, declaration: &ast::VarDecl) {
            if declaration.kind == ast::VarDeclKind::Const {
                for item in &declaration.decls {
                    if matches!(&item.name, ast::Pat::Ident(name) if name.id.to_id() == *self.id) {
                        self.value = item.init.clone();
                    }
                }
            }
            declaration.visit_children_with(self);
        }
    }
    let mut find = Find { id, value: None };
    module.visit_with(&mut find);
    find.value
}

// An object-valued const can still escape or be mutated. Only its use inside
// static macro arguments is sufficient proof; aliases are conservatively refused.
fn only_macro_arguments(module: &ast::Module, id: &ast::Id) -> bool {
    struct Uses<'a> {
        module: &'a ast::Module,
        id: &'a ast::Id,
        allowed: bool,
        safe: bool,
    }
    impl Visit for Uses<'_> {
        fn visit_import_decl(&mut self, _: &ast::ImportDecl) {}
        fn visit_ts_type(&mut self, _: &ast::TsType) {}
        fn visit_prop(&mut self, prop: &ast::Prop) {
            if let ast::Prop::Shorthand(name) = prop {
                self.visit_expr(&ast::Expr::Ident(name.clone()));
            } else {
                prop.visit_children_with(self);
            }
        }
        fn visit_call_expr(&mut self, call: &ast::CallExpr) {
            if matches!(&call.callee, ast::Callee::Expr(callee) if binding(self.module, callee).is_some())
            {
                let before = self.allowed;
                self.allowed = true;
                call.args.visit_with(self);
                self.allowed = before;
            } else {
                let before = self.allowed;
                self.allowed = false;
                call.visit_children_with(self);
                self.allowed = before;
            }
        }
        fn visit_expr(&mut self, expr: &ast::Expr) {
            if matches!(expr, ast::Expr::Ident(name) if name.to_id() == *self.id) && !self.allowed {
                self.safe = false;
            }
            expr.visit_children_with(self);
        }
    }
    let mut uses = Uses {
        module,
        id,
        allowed: false,
        safe: true,
    };
    module.visit_with(&mut uses);
    uses.safe
}

pub(super) fn source(
    expr: &ast::Expr,
    module: &ast::Module,
    calls: &[Invocation],
    active: &mut Vec<ast::Id>,
) -> Result<String> {
    let descend = |expr: &ast::Expr, active: &mut Vec<ast::Id>| source(expr, module, calls, active);
    Ok(match expr {
        ast::Expr::Lit(ast::Lit::Str(value)) => serde_json::to_string(
            value
                .value
                .as_str()
                .context("non-Unicode static macro string")?,
        )?,
        ast::Expr::Lit(ast::Lit::Num(value)) if value.value.is_finite() => {
            serde_json::to_string(&value.value)?
        }
        ast::Expr::Lit(ast::Lit::Bool(value)) => value.value.to_string(),
        ast::Expr::Lit(ast::Lit::Null(_)) => "null".to_string(),
        ast::Expr::Paren(value) => descend(&value.expr, active)?,
        ast::Expr::TsAs(value) => descend(&value.expr, active)?,
        ast::Expr::TsSatisfies(value) => descend(&value.expr, active)?,
        ast::Expr::TsNonNull(value) => descend(&value.expr, active)?,
        ast::Expr::TsTypeAssertion(value) => descend(&value.expr, active)?,
        ast::Expr::Ident(name) => {
            let id = name.to_id();
            if active.contains(&id) {
                bail!("cyclic static macro argument {:?}", name.sym);
            }
            let value = constant(module, &id).with_context(|| {
                format!("macro argument {:?} is not statically known", name.sym)
            })?;
            if !matches!(value.as_ref(), ast::Expr::Lit(_)) && !only_macro_arguments(module, &id) {
                bail!(
                    "macro argument {:?} can escape or change before expansion",
                    name.sym
                );
            }
            active.push(id);
            let result = descend(&value, active);
            active.pop();
            result?
        }
        ast::Expr::Call(call) => {
            let index = calls
                .iter()
                .position(|item| item.identity == call.ctxt)
                .context("macro argument calls a runtime function and is not statically known")?;
            format!("JSON.parse(__macroValues[{index}])")
        }
        ast::Expr::Array(array) => {
            let values = array
                .elems
                .iter()
                .map(|item| {
                    let item = item
                        .as_ref()
                        .context("array hole is not a static JSON macro argument")?;
                    if item.spread.is_some() {
                        bail!("spread is not a static JSON macro argument");
                    }
                    descend(&item.expr, active)
                })
                .collect::<Result<Vec<_>>>()?;
            format!("[{}]", values.join(","))
        }
        ast::Expr::Object(object) => {
            let values = object
                .props
                .iter()
                .map(|property| {
                    let ast::PropOrSpread::Prop(property) = property else {
                        bail!("object spread is not a static JSON macro argument")
                    };
                    let (key, value) = match property.as_ref() {
                        ast::Prop::KeyValue(property) => {
                            let key = match &property.key {
                                ast::PropName::Ident(name) => {
                                    serde_json::to_string(name.sym.as_ref())?
                                }
                                ast::PropName::Str(name) => serde_json::to_string(
                                    name.value
                                        .as_str()
                                        .context("non-Unicode macro argument key")?,
                                )?,
                                ast::PropName::Num(name) => {
                                    serde_json::to_string(&name.value.to_string())?
                                }
                                ast::PropName::Computed(name) => {
                                    format!("[{}]", descend(&name.expr, active)?)
                                }
                                _ => bail!("non-JSON static macro argument key"),
                            };
                            (key, descend(&property.value, active)?)
                        }
                        ast::Prop::Shorthand(name) => (
                            format!("[{}]", serde_json::to_string(name.sym.as_ref())?),
                            descend(&ast::Expr::Ident(name.clone()), active)?,
                        ),
                        _ => bail!("macro argument methods/accessors are not JSON values"),
                    };
                    Ok(format!("{key}:{value}"))
                })
                .collect::<Result<Vec<_>>>()?;
            format!("({{{}}})", values.join(","))
        }
        ast::Expr::Unary(unary) => {
            let op = match unary.op {
                ast::UnaryOp::Minus => "-",
                ast::UnaryOp::Plus => "+",
                ast::UnaryOp::Bang => "!",
                ast::UnaryOp::Tilde => "~",
                ast::UnaryOp::TypeOf => "typeof ",
                _ => bail!("macro argument operator is not statically known"),
            };
            format!("({op}({}))", descend(&unary.arg, active)?)
        }
        ast::Expr::Bin(binary) => {
            let op = match binary.op {
                ast::BinaryOp::Add => "+",
                ast::BinaryOp::Sub => "-",
                ast::BinaryOp::Mul => "*",
                ast::BinaryOp::Div => "/",
                ast::BinaryOp::Mod => "%",
                ast::BinaryOp::Exp => "**",
                ast::BinaryOp::EqEq => "==",
                ast::BinaryOp::NotEq => "!=",
                ast::BinaryOp::EqEqEq => "===",
                ast::BinaryOp::NotEqEq => "!==",
                ast::BinaryOp::Lt => "<",
                ast::BinaryOp::LtEq => "<=",
                ast::BinaryOp::Gt => ">",
                ast::BinaryOp::GtEq => ">=",
                ast::BinaryOp::LogicalAnd => "&&",
                ast::BinaryOp::LogicalOr => "||",
                ast::BinaryOp::NullishCoalescing => "??",
                _ => bail!("unsupported static macro argument operator"),
            };
            format!(
                "(({}){op}({}))",
                descend(&binary.left, active)?,
                descend(&binary.right, active)?
            )
        }
        ast::Expr::Cond(condition) => format!(
            "(({})?({}):({}))",
            descend(&condition.test, active)?,
            descend(&condition.cons, active)?,
            descend(&condition.alt, active)?
        ),
        ast::Expr::Tpl(template) => {
            let mut parts = Vec::new();
            for (index, quasi) in template.quasis.iter().enumerate() {
                let text = quasi
                    .cooked
                    .as_ref()
                    .and_then(|text| text.as_str())
                    .context("invalid static macro template")?;
                parts.push(serde_json::to_string(text)?);
                if let Some(expr) = template.exprs.get(index) {
                    parts.push(format!("String({})", descend(expr, active)?));
                }
            }
            format!("({})", parts.join("+"))
        }
        _ => bail!("macro argument is not statically known JSON"),
    })
}
