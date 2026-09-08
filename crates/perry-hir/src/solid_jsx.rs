//! Solid universal JSX expansion before ordinary closure/accessor HIR lowering.
//!
//! Native nodes are constructed once. Property getters and child accessors keep
//! signal reads inside Solid effects; component render-prop functions stay values.

use std::collections::BTreeSet;

use swc_common::{Spanned, DUMMY_SP};
use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitMut, VisitMutWith, VisitWith};

/// Expand JSX for an explicitly selected universal renderer. Returns `None`
/// without cloning when the module contains no JSX.
pub fn lower_solid_jsx(module: &ast::Module, runtime: &str) -> Option<ast::Module> {
    #[derive(Default)]
    struct Names {
        names: BTreeSet<String>,
        jsx: bool,
    }
    impl Visit for Names {
        fn visit_ident(&mut self, ident: &ast::Ident) {
            self.names.insert(ident.sym.to_string());
        }
        fn visit_jsx_element(&mut self, element: &ast::JSXElement) {
            self.jsx = true;
            element.visit_children_with(self);
        }
        fn visit_jsx_fragment(&mut self, fragment: &ast::JSXFragment) {
            self.jsx = true;
            fragment.visit_children_with(self);
        }
    }
    let mut names = Names::default();
    module.visit_with(&mut names);
    if !names.jsx {
        return None;
    }
    let prefix = (0..)
        .map(|n| format!("__perry_solid_{n}_"))
        .find(|prefix| !names.names.iter().any(|name| name.starts_with(prefix)))
        .expect("finite source identifiers leave a free helper prefix");
    let mut lowering = SolidJsx {
        prefix,
        next: 0,
        helpers: BTreeSet::new(),
    };
    let mut result = module.clone();
    result.visit_mut_with(&mut lowering);
    if lowering.helpers.is_empty() {
        return Some(result);
    }
    let specifiers = lowering
        .helpers
        .iter()
        .map(|name| {
            ast::ImportSpecifier::Named(ast::ImportNamedSpecifier {
                span: DUMMY_SP,
                local: ident(&format!("{}{name}", lowering.prefix)),
                imported: Some(ast::ModuleExportName::Ident(ident(name))),
                is_type_only: false,
            })
        })
        .collect();
    result.body.insert(
        0,
        ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(ast::ImportDecl {
            span: DUMMY_SP,
            specifiers,
            src: Box::new(ast::Str {
                span: DUMMY_SP,
                value: runtime.into(),
                raw: None,
            }),
            type_only: false,
            with: None,
            phase: Default::default(),
        })),
    );
    Some(result)
}

struct SolidJsx {
    prefix: String,
    next: usize,
    helpers: BTreeSet<String>,
}

fn ident(name: &str) -> ast::Ident {
    ast::Ident::new(name.into(), DUMMY_SP, Default::default())
}

fn string(value: &str) -> ast::Expr {
    ast::Expr::Lit(ast::Lit::Str(ast::Str {
        span: DUMMY_SP,
        value: value.into(),
        raw: None,
    }))
}

fn call(callee: ast::Expr, args: Vec<ast::Expr>) -> ast::Expr {
    ast::Expr::Call(ast::CallExpr {
        callee: ast::Callee::Expr(Box::new(callee)),
        args: args.into_iter().map(|expr| expr.into()).collect(),
        ..Default::default()
    })
}

fn arrow(value: ast::Expr) -> ast::Expr {
    ast::Expr::Arrow(ast::ArrowExpr {
        body: Box::new(ast::BlockStmtOrExpr::Expr(Box::new(value))),
        ..Default::default()
    })
}

fn statement(expr: ast::Expr) -> ast::Stmt {
    ast::Stmt::Expr(ast::ExprStmt {
        span: expr.span(),
        expr: Box::new(expr),
    })
}

fn binding(name: ast::Ident, value: ast::Expr) -> ast::Stmt {
    ast::Stmt::Decl(ast::Decl::Var(Box::new(ast::VarDecl {
        kind: ast::VarDeclKind::Const,
        decls: vec![ast::VarDeclarator {
            span: DUMMY_SP,
            name: ast::Pat::Ident(name.into()),
            init: Some(Box::new(value)),
            definite: false,
        }],
        ..Default::default()
    })))
}

fn block_expr(mut statements: Vec<ast::Stmt>, result: ast::Expr) -> ast::Expr {
    statements.push(ast::Stmt::Return(ast::ReturnStmt {
        span: DUMMY_SP,
        arg: Some(Box::new(result)),
    }));
    call(
        ast::Expr::Arrow(ast::ArrowExpr {
            body: Box::new(ast::BlockStmtOrExpr::BlockStmt(ast::BlockStmt {
                stmts: statements,
                ..Default::default()
            })),
            ..Default::default()
        }),
        vec![],
    )
}

fn property(name: &str, value: ast::Expr, getter: bool) -> ast::PropOrSpread {
    let key = ast::PropName::Str(ast::Str {
        span: DUMMY_SP,
        value: name.into(),
        raw: None,
    });
    let prop = if getter {
        ast::Prop::Getter(ast::GetterProp {
            span: value.span(),
            key,
            type_ann: None,
            body: Some(ast::BlockStmt {
                stmts: vec![ast::Stmt::Return(ast::ReturnStmt {
                    span: value.span(),
                    arg: Some(Box::new(value)),
                })],
                ..Default::default()
            }),
        })
    } else {
        ast::Prop::KeyValue(ast::KeyValueProp {
            key,
            value: Box::new(value),
        })
    };
    ast::PropOrSpread::Prop(Box::new(prop))
}

fn object(props: Vec<ast::PropOrSpread>) -> ast::Expr {
    ast::Expr::Object(ast::ObjectLit {
        span: DUMMY_SP,
        props,
    })
}

fn array(elements: Vec<ast::Expr>) -> ast::Expr {
    ast::Expr::Array(ast::ArrayLit {
        span: DUMMY_SP,
        elems: elements.into_iter().map(|expr| Some(expr.into())).collect(),
    })
}

fn is_static_value(expr: &ast::Expr) -> bool {
    matches!(
        expr,
        ast::Expr::Lit(_) | ast::Expr::Arrow(_) | ast::Expr::Fn(_)
    )
}

fn contains_jsx(expr: &ast::Expr) -> bool {
    struct Find(bool);
    impl Visit for Find {
        fn visit_jsx_element(&mut self, _: &ast::JSXElement) {
            self.0 = true;
        }
        fn visit_jsx_fragment(&mut self, _: &ast::JSXFragment) {
            self.0 = true;
        }
    }
    let mut find = Find(false);
    expr.visit_with(&mut find);
    find.0
}

fn boolean(expr: ast::Expr) -> ast::Expr {
    ast::Expr::Unary(ast::UnaryExpr {
        span: expr.span(),
        op: ast::UnaryOp::Bang,
        arg: Box::new(ast::Expr::Unary(ast::UnaryExpr {
            span: expr.span(),
            op: ast::UnaryOp::Bang,
            arg: Box::new(expr),
        })),
    })
}

impl SolidJsx {
    fn helper(&mut self, name: &str, args: Vec<ast::Expr>) -> ast::Expr {
        self.helpers.insert(name.to_string());
        call(
            ast::Expr::Ident(ident(&format!("{}{name}", self.prefix))),
            args,
        )
    }

    fn temporary(&mut self) -> ast::Ident {
        let name = ident(&format!("{}node_{}", self.prefix, self.next));
        self.next += 1;
        name
    }

    fn expression(&mut self, mut expression: ast::Expr) -> ast::Expr {
        expression.visit_mut_with(self);
        expression
    }

    fn getter_expression(&mut self, mut expression: ast::Expr) -> ast::Expr {
        let condition = match &mut expression {
            ast::Expr::Cond(cond) if contains_jsx(&cond.cons) || contains_jsx(&cond.alt) => {
                Some(&mut cond.test)
            }
            ast::Expr::Bin(binary)
                if binary.op == ast::BinaryOp::LogicalAnd && contains_jsx(&binary.right) =>
            {
                Some(&mut binary.left)
            }
            _ => None,
        };
        if let Some(condition) = condition {
            let test = self.expression(*condition.clone());
            let memo = self.helper("memo", vec![arrow(boolean(test))]);
            *condition = Box::new(call(memo, vec![]));
        }
        self.expression(expression)
    }

    fn child_accessor(&mut self, expr: ast::Expr) -> ast::Expr {
        // A truthy-to-truthy update must retain an existing conditional branch.
        // Track the condition's boolean value separately from the branch factory.
        let mut expr = expr;
        let condition = match &mut expr {
            ast::Expr::Cond(cond) if contains_jsx(&cond.cons) || contains_jsx(&cond.alt) => {
                Some(&mut cond.test)
            }
            ast::Expr::Bin(binary)
                if binary.op == ast::BinaryOp::LogicalAnd && contains_jsx(&binary.right) =>
            {
                Some(&mut binary.left)
            }
            _ => None,
        };
        let mut setup = Vec::new();
        if let Some(condition) = condition {
            let value = self.expression(*condition.clone());
            let memo = self.helper("memo", vec![arrow(boolean(value))]);
            let name = self.temporary();
            setup.push(binding(name.clone(), memo));
            *condition = Box::new(call(ast::Expr::Ident(name), vec![]));
        }
        let accessor = arrow(self.expression(expr));
        if setup.is_empty() {
            accessor
        } else {
            block_expr(setup, accessor)
        }
    }

    fn element_name(&mut self, name: &ast::JSXElementName) -> (ast::Expr, bool) {
        match name {
            ast::JSXElementName::Ident(name) if name.sym.starts_with(char::is_lowercase) => {
                (string(&name.sym), true)
            }
            ast::JSXElementName::Ident(name) => (ast::Expr::Ident(name.clone()), false),
            ast::JSXElementName::JSXMemberExpr(member) => (Self::member(member), false),
            ast::JSXElementName::JSXNamespacedName(name) => {
                (string(&format!("{}:{}", name.ns.sym, name.name.sym)), true)
            }
        }
    }

    fn member(member: &ast::JSXMemberExpr) -> ast::Expr {
        ast::Expr::Member(ast::MemberExpr {
            span: member.span,
            obj: Box::new(match &member.obj {
                ast::JSXObject::Ident(name) => ast::Expr::Ident(name.clone()),
                ast::JSXObject::JSXMemberExpr(parent) => Self::member(parent),
            }),
            prop: ast::MemberProp::Ident(member.prop.clone()),
        })
    }

    fn attribute_value(&mut self, value: &ast::JSXAttrValue) -> ast::Expr {
        match value {
            ast::JSXAttrValue::Str(value) => ast::Expr::Lit(ast::Lit::Str(value.clone())),
            ast::JSXAttrValue::JSXExprContainer(container) => match &container.expr {
                ast::JSXExpr::Expr(expr) => self.getter_expression(*expr.clone()),
                ast::JSXExpr::JSXEmptyExpr(_) => ast::Expr::Ident(ident("undefined")),
            },
            ast::JSXAttrValue::JSXElement(element) => self.element(element),
            ast::JSXAttrValue::JSXFragment(fragment) => self.fragment(fragment),
        }
    }

    fn ref_value(&mut self, value: ast::Expr) -> ast::Expr {
        let target = ast::AssignTarget::try_from(Box::new(value.clone())).ok();
        let node = self.temporary();
        let current = self.temporary();
        let invoke = call(
            ast::Expr::Ident(current.clone()),
            vec![ast::Expr::Ident(node.clone())],
        );
        let action = if let Some(target) = target {
            let assign = ast::Expr::Assign(ast::AssignExpr {
                span: DUMMY_SP,
                op: ast::AssignOp::Assign,
                left: target,
                right: Box::new(ast::Expr::Ident(node.clone())),
            });
            ast::Expr::Cond(ast::CondExpr {
                span: DUMMY_SP,
                test: Box::new(ast::Expr::Bin(ast::BinExpr {
                    span: DUMMY_SP,
                    op: ast::BinaryOp::EqEqEq,
                    left: Box::new(ast::Expr::Unary(ast::UnaryExpr {
                        span: DUMMY_SP,
                        op: ast::UnaryOp::TypeOf,
                        arg: Box::new(ast::Expr::Ident(current.clone())),
                    })),
                    right: Box::new(string("function")),
                })),
                cons: Box::new(invoke),
                alt: Box::new(assign),
            })
        } else {
            invoke
        };
        let callback = ast::Expr::Arrow(ast::ArrowExpr {
            body: Box::new(ast::BlockStmtOrExpr::BlockStmt(ast::BlockStmt {
                stmts: vec![binding(current, value), statement(action)],
                ..Default::default()
            })),
            ..Default::default()
        });
        // Universal `use` invokes its callback untracked. Evaluating both the
        // reference expression and its callback there avoids replaying refs when
        // they happen to read a signal during widget construction.
        let untracked = self.helper("use", vec![callback, ast::Expr::Ident(node.clone())]);
        ast::Expr::Arrow(ast::ArrowExpr {
            params: vec![ast::Pat::Ident(node.into())],
            body: Box::new(ast::BlockStmtOrExpr::Expr(Box::new(untracked))),
            ..Default::default()
        })
    }

    fn child(&mut self, child: &ast::JSXElementChild, native: bool) -> Option<ast::Expr> {
        match child {
            ast::JSXElementChild::JSXText(text) => {
                let text = crate::jsx::normalize_jsx_text(&text.value);
                (!text.is_empty()).then(|| string(&text))
            }
            ast::JSXElementChild::JSXElement(element) => Some(self.element(element)),
            ast::JSXElementChild::JSXFragment(fragment) => Some(self.fragment(fragment)),
            ast::JSXElementChild::JSXExprContainer(container) => match &container.expr {
                ast::JSXExpr::JSXEmptyExpr(_) => None,
                ast::JSXExpr::Expr(expr) => {
                    let value = *expr.clone();
                    Some(
                        if native
                            && !is_static_value(&value)
                            && !matches!(
                                value,
                                ast::Expr::JSXElement(_) | ast::Expr::JSXFragment(_)
                            )
                        {
                            self.child_accessor(value)
                        } else {
                            if native {
                                self.expression(value)
                            } else {
                                self.getter_expression(value)
                            }
                        },
                    )
                }
            },
            ast::JSXElementChild::JSXSpreadChild(child) => {
                let expr = self.expression(*child.expr.clone());
                Some(if native { arrow(expr) } else { expr })
            }
        }
    }

    fn element(&mut self, element: &ast::JSXElement) -> ast::Expr {
        let (name, native) = self.element_name(&element.opening.name);
        let mut chunks = Vec::new();
        let mut props = Vec::new();
        let mut has_spread = false;
        for attribute in &element.opening.attrs {
            match attribute {
                ast::JSXAttrOrSpread::SpreadElement(spread) => {
                    has_spread = true;
                    if !props.is_empty() {
                        chunks.push(object(std::mem::take(&mut props)));
                    }
                    let source = self.expression(*spread.expr.clone());
                    chunks.push(arrow(source));
                }
                ast::JSXAttrOrSpread::JSXAttr(attribute) => {
                    let key = match &attribute.name {
                        ast::JSXAttrName::Ident(name) => name.sym.to_string(),
                        ast::JSXAttrName::JSXNamespacedName(name) => {
                            format!("{}:{}", name.ns.sym, name.name.sym)
                        }
                    };
                    let mut value = attribute
                        .value
                        .as_ref()
                        .map(|value| self.attribute_value(value))
                        .unwrap_or_else(|| {
                            ast::Expr::Lit(ast::Lit::Bool(ast::Bool {
                                span: DUMMY_SP,
                                value: true,
                            }))
                        });
                    if key == "ref" {
                        value = self.ref_value(value);
                    }
                    let getter = !is_static_value(&value);
                    props.push(property(&key, value, getter));
                }
            }
        }
        let mut children = element
            .children
            .iter()
            .filter_map(|child| self.child(child, native))
            .collect::<Vec<_>>();
        if !children.is_empty() {
            let children = if children.len() == 1 {
                children.remove(0)
            } else {
                array(children)
            };
            let getter = !native && !is_static_value(&children);
            props.push(property("children", children, getter));
        }
        if !props.is_empty() || chunks.is_empty() {
            chunks.push(object(props));
        }
        let props = if chunks.len() == 1 && !has_spread {
            chunks.remove(0)
        } else {
            self.helper("mergeProps", chunks)
        };
        if native {
            let node = self.temporary();
            let create = self.helper("createElement", vec![name]);
            let spread = self.helper("spread", vec![ast::Expr::Ident(node.clone()), props]);
            block_expr(
                vec![binding(node.clone(), create), statement(spread)],
                ast::Expr::Ident(node),
            )
        } else {
            self.helper("createComponent", vec![name, props])
        }
    }

    fn fragment(&mut self, fragment: &ast::JSXFragment) -> ast::Expr {
        array(
            fragment
                .children
                .iter()
                .filter_map(|child| self.child(child, true))
                .collect(),
        )
    }
}

impl VisitMut for SolidJsx {
    fn visit_mut_expr(&mut self, expression: &mut ast::Expr) {
        match expression {
            ast::Expr::JSXElement(element) => *expression = self.element(element),
            ast::Expr::JSXFragment(fragment) => *expression = self.fragment(fragment),
            _ => expression.visit_mut_children_with(self),
        }
    }
}
