//! Scope-aware build-time expression substitution. Cached ASTs stay unchanged.

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use swc_common::{Globals, Mark, SyntaxContext, GLOBALS};
use swc_ecma_ast as ast;
use swc_ecma_visit::{VisitMut, VisitMutWith};

#[derive(Clone, Debug, Default)]
pub struct Defines {
    expressions: BTreeMap<String, Box<ast::Expr>>,
}

impl Defines {
    pub fn parse(values: &BTreeMap<String, String>) -> Result<Self> {
        let mut expressions = BTreeMap::new();
        for (key, value) in values {
            let key_expr =
                parse_expression(key).with_context(|| format!("invalid define key {key:?}"))?;
            if dotted_name(&key_expr).as_deref() != Some(key) {
                bail!("invalid define key {key:?}: expected an identifier or dotted identifier");
            }
            let expression = parse_expression(value)
                .with_context(|| format!("invalid define value for {key}"))?;
            if !valid_value(&expression) {
                bail!("invalid define value for {key}: expected JSON or an identifier expression");
            }
            expressions.insert(key.clone(), expression);
        }
        Ok(Self { expressions })
    }

    pub fn apply(&self, module: &ast::Module) -> Option<ast::Module> {
        if self.expressions.is_empty() {
            return None;
        }
        Some(GLOBALS.set(&Globals::new(), || {
            let unresolved = Mark::new();
            let mut module = module.clone();
            module.visit_mut_with(&mut swc_ecma_transforms_base::resolver(
                unresolved,
                Mark::new(),
                false,
            ));
            module.visit_mut_with(&mut Substitute {
                defines: self,
                unresolved: SyntaxContext::empty().apply_mark(unresolved),
            });
            // HIR uses lexical names; resolver contexts must not escape GLOBALS.
            module.visit_mut_with(&mut ClearContexts);
            module
        }))
    }
}

fn parse_expression(source: &str) -> Result<Box<ast::Expr>> {
    let module = crate::parse_typescript(&format!("({source});"), "define.js")?;
    let [ast::ModuleItem::Stmt(ast::Stmt::Expr(stmt))] = module.body.as_slice() else {
        bail!("expected one expression");
    };
    let ast::Expr::Paren(paren) = stmt.expr.as_ref() else {
        bail!("expected one expression")
    };
    Ok(paren.expr.clone())
}

fn dotted_name(expr: &ast::Expr) -> Option<String> {
    match expr {
        ast::Expr::Ident(ident) => Some(ident.sym.to_string()),
        ast::Expr::Member(member) => {
            let prop = match &member.prop {
                ast::MemberProp::Ident(prop) => prop.sym.as_ref(),
                ast::MemberProp::Computed(key) => match key.expr.as_ref() {
                    ast::Expr::Lit(ast::Lit::Str(prop)) => prop.value.as_str()?,
                    _ => return None,
                },
                _ => return None,
            };
            Some(format!("{}.{}", dotted_name(&member.obj)?, prop))
        }
        _ => None,
    }
}

fn valid_value(expr: &ast::Expr) -> bool {
    dotted_name(expr).is_some() || valid_json_value(expr)
}

fn valid_json_value(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Lit(ast::Lit::Str(_) | ast::Lit::Bool(_) | ast::Lit::Num(_) | ast::Lit::Null(_)) => true,
        ast::Expr::Unary(unary) => matches!(unary.op, ast::UnaryOp::Minus | ast::UnaryOp::Plus)
            && matches!(unary.arg.as_ref(), ast::Expr::Lit(ast::Lit::Num(_))),
        ast::Expr::Array(array) => array.elems.iter().all(|item| item.as_ref().is_some_and(|item| item.spread.is_none() && valid_json_value(&item.expr))),
        ast::Expr::Object(object) => object.props.iter().all(|prop| matches!(prop,
            ast::PropOrSpread::Prop(prop) if matches!(prop.as_ref(), ast::Prop::KeyValue(kv)
                if matches!(kv.key, ast::PropName::Ident(_) | ast::PropName::Str(_) | ast::PropName::Num(_)) && valid_json_value(&kv.value)))),
        _ => false,
    }
}

struct Substitute<'a> {
    defines: &'a Defines,
    unresolved: SyntaxContext,
}

impl Substitute<'_> {
    fn replacement(&self, expr: &ast::Expr) -> Option<Box<ast::Expr>> {
        let mut root = expr;
        while let ast::Expr::Member(member) = root {
            root = &member.obj;
        }
        let ast::Expr::Ident(root) = root else {
            return None;
        };
        if root.ctxt != self.unresolved {
            return None;
        }
        self.defines.expressions.get(&dotted_name(expr)?).cloned()
    }
}

impl VisitMut for Substitute<'_> {
    fn visit_mut_expr(&mut self, expr: &mut ast::Expr) {
        if let Some(replacement) = self.replacement(expr) {
            *expr = *replacement;
            return; // Replacement expressions are not recursively substituted.
        }
        match expr {
            ast::Expr::Assign(assign) => {
                assign.left.visit_mut_with(self);
                assign.right.visit_mut_with(self);
                return;
            }
            ast::Expr::Update(_) => return,
            ast::Expr::Unary(unary) if unary.op == ast::UnaryOp::Delete => return,
            _ => {}
        }
        expr.visit_mut_children_with(self);
        if let ast::Expr::Unary(unary) = expr {
            if unary.op == ast::UnaryOp::TypeOf {
                let kind = match unary.arg.as_ref() {
                    ast::Expr::Lit(ast::Lit::Str(_)) => Some("string"),
                    ast::Expr::Lit(ast::Lit::Bool(_)) => Some("boolean"),
                    ast::Expr::Lit(ast::Lit::Num(_)) => Some("number"),
                    ast::Expr::Lit(ast::Lit::Null(_)) => Some("object"),
                    value @ (ast::Expr::Object(_) | ast::Expr::Array(_))
                        if valid_json_value(value) =>
                    {
                        Some("object")
                    }
                    ast::Expr::Ident(id)
                        if id.sym == "undefined"
                            && (id.ctxt == self.unresolved
                                || id.ctxt == SyntaxContext::empty()) =>
                    {
                        Some("undefined")
                    }
                    _ => None,
                };
                if let Some(kind) = kind {
                    *expr = ast::Expr::Lit(ast::Lit::Str(ast::Str {
                        span: unary.span,
                        value: kind.into(),
                        raw: None,
                    }));
                }
            }
        }
        if let ast::Expr::Bin(binary) = expr {
            if matches!(binary.op, ast::BinaryOp::EqEqEq | ast::BinaryOp::NotEqEq) {
                let equal = match (binary.left.as_ref(), binary.right.as_ref()) {
                    (ast::Expr::Lit(ast::Lit::Str(a)), ast::Expr::Lit(ast::Lit::Str(b))) => {
                        Some(a.value == b.value)
                    }
                    (ast::Expr::Lit(ast::Lit::Bool(a)), ast::Expr::Lit(ast::Lit::Bool(b))) => {
                        Some(a.value == b.value)
                    }
                    (ast::Expr::Lit(ast::Lit::Num(a)), ast::Expr::Lit(ast::Lit::Num(b))) => {
                        Some(a.value == b.value)
                    }
                    _ => None,
                };
                if let Some(equal) = equal {
                    *expr = ast::Expr::Lit(ast::Lit::Bool(ast::Bool {
                        span: binary.span,
                        value: if binary.op == ast::BinaryOp::EqEqEq {
                            equal
                        } else {
                            !equal
                        },
                    }));
                }
            }
        }
        if let ast::Expr::Cond(conditional) = expr {
            if let ast::Expr::Lit(ast::Lit::Bool(test)) = conditional.test.as_ref() {
                *expr = *if test.value {
                    conditional.cons.clone()
                } else {
                    conditional.alt.clone()
                };
            }
        }
    }

    fn visit_mut_prop(&mut self, prop: &mut ast::Prop) {
        if let ast::Prop::Shorthand(ident) = prop {
            if let Some(value) = self.replacement(&ast::Expr::Ident(ident.clone())) {
                *prop = ast::Prop::KeyValue(ast::KeyValueProp {
                    key: ast::PropName::Ident(ident.clone().into()),
                    value,
                });
                return;
            }
        }
        prop.visit_mut_children_with(self);
    }

    fn visit_mut_ts_type(&mut self, _: &mut ast::TsType) {}
}

struct ClearContexts;
impl VisitMut for ClearContexts {
    fn visit_mut_syntax_context(&mut self, ctxt: &mut SyntaxContext) {
        *ctxt = SyntaxContext::empty();
    }
    fn visit_mut_span(&mut self, _: &mut swc_common::Span) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_keys_and_expressions() {
        for (key, value) in [
            ("A-B", "1"),
            ("a['b']", "1"),
            ("A", "foo()"),
            ("A", "1); evil(); (2"),
        ] {
            assert!(Defines::parse(&BTreeMap::from([(key.into(), value.into())])).is_err());
        }
        Defines::parse(&BTreeMap::from([(
            "a.b".into(),
            r#"{"models":[1,true,null,"x"]}"#.into(),
        )]))
        .unwrap();
    }

    #[test]
    fn replaces_reads_and_shorthand_but_preserves_bindings_and_writes() {
        let defines = Defines::parse(&BTreeMap::from([
            ("VERSION".into(), "\"1.18.30\"".into()),
            ("process.env.X".into(), "false".into()),
        ]))
        .unwrap();
        let module = crate::parse_typescript("console.log(VERSION, { VERSION }, typeof VERSION, process.env.X); function f(VERSION: string, process: any) { return [VERSION, process.env.X]; } VERSION = 'write';", "test.ts").unwrap();
        let transformed = defines.apply(&module).unwrap();
        let dump = format!("{transformed:?}");
        assert_eq!(dump.matches("value: \"1.18.30\"").count(), 2);
        assert!(dump.contains("value: \"string\""));
        assert_eq!(dump.matches("value: false").count(), 1);
        assert!(!format!("{module:?}").contains("1.18.30"));
    }

    #[test]
    fn ambient_declarations_are_not_runtime_bindings() {
        let defines = Defines::parse(&BTreeMap::from([("VERSION".into(), "42".into())])).unwrap();
        let module = crate::parse_typescript(
            "declare const VERSION: number; console.log(VERSION);",
            "test.ts",
        )
        .unwrap();
        let dump = format!("{:?}", defines.apply(&module).unwrap());
        assert!(dump.contains("value: 42.0"), "{dump}");
    }

    #[test]
    fn computed_reads_are_replaced_without_dropping_typeof_effects() {
        let defines = Defines::parse(&BTreeMap::from([
            ("process.env.X".into(), "'defined'".into()),
            ("KEY".into(), "'key'".into()),
        ]))
        .unwrap();
        let module = crate::parse_typescript(
            "console.log(process['env']['X'], typeof { x: sideEffect() }); object[KEY] = 1; process.env.X = 'write';",
            "test.ts",
        ).unwrap();
        let dump = format!("{:?}", defines.apply(&module).unwrap());
        assert_eq!(dump.matches("value: \"defined\"").count(), 1);
        assert_eq!(dump.matches("value: \"key\"").count(), 1);
        assert!(dump.contains("sideEffect"));
        assert!(dump.contains("typeof"), "{dump}");
    }
}
