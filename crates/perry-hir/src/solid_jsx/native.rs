use super::*;

struct NativeElement {
    node: ast::Ident,
    declarations: Vec<ast::Stmt>,
    statements: Vec<ast::Stmt>,
    dynamics: Vec<(ast::Ident, String, ast::Expr)>,
}

fn null() -> ast::Expr {
    ast::Expr::Lit(ast::Lit::Null(ast::Null { span: DUMMY_SP }))
}

fn bool_value(value: bool) -> ast::Expr {
    ast::Expr::Lit(ast::Lit::Bool(ast::Bool {
        span: DUMMY_SP,
        value,
    }))
}

fn field(object: &ast::Ident, key: &str) -> ast::Expr {
    ast::Expr::Member(ast::MemberExpr {
        span: DUMMY_SP,
        obj: Box::new(ast::Expr::Ident(object.clone())),
        prop: ast::MemberProp::Ident(ast::IdentName::new(key.into(), DUMMY_SP)),
    })
}

fn text_child(child: &ast::JSXElementChild) -> Option<String> {
    match child {
        ast::JSXElementChild::JSXText(text) => Some(crate::jsx::normalize_jsx_text(&text.value)),
        ast::JSXElementChild::JSXExprContainer(container) => match &container.expr {
            ast::JSXExpr::Expr(expr) => match &**expr {
                ast::Expr::Lit(ast::Lit::Str(s)) => Some(s.value.to_string_lossy().into_owned()),
                ast::Expr::Lit(ast::Lit::Num(n)) => Some(n.value.to_string()),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

impl SolidJsx {
    pub(super) fn native_element(&mut self, element: &ast::JSXElement) -> ast::Expr {
        let mut parts = self.native_parts(element);
        parts.declarations.append(&mut parts.statements);
        if !parts.dynamics.is_empty() {
            let effect = self.property_effect(parts.dynamics);
            parts.declarations.push(statement(effect));
        }
        block_expr(parts.declarations, ast::Expr::Ident(parts.node))
    }

    fn native_parts(&mut self, element: &ast::JSXElement) -> NativeElement {
        let node = self.temporary();
        let (name, _) = self.element_name(&element.opening.name);
        let create = self.helper("createElement", vec![name]);
        let mut result = NativeElement {
            node: node.clone(),
            declarations: vec![binding(node.clone(), create)],
            statements: Vec::new(),
            dynamics: Vec::new(),
        };
        let has_spread = element
            .opening
            .attrs
            .iter()
            .any(|a| matches!(a, ast::JSXAttrOrSpread::SpreadElement(_)));
        let has_children = element.children.iter().any(|child| match child {
            ast::JSXElementChild::JSXText(t) => {
                !crate::jsx::normalize_jsx_text(&t.value).is_empty()
            }
            ast::JSXElementChild::JSXExprContainer(c) => matches!(c.expr, ast::JSXExpr::Expr(_)),
            _ => true,
        });
        let mut chunks = Vec::new();
        let mut props = Vec::new();
        let mut children_prop = None;
        for attr in &element.opening.attrs {
            let attr = match attr {
                ast::JSXAttrOrSpread::SpreadElement(spread) => {
                    if !props.is_empty() {
                        chunks.push(object(std::mem::take(&mut props)));
                    }
                    let dynamic = is_dynamic(&spread.expr);
                    let source = self.expression(*spread.expr.clone());
                    chunks.push(if dynamic { arrow(source) } else { source });
                    continue;
                }
                ast::JSXAttrOrSpread::JSXAttr(attr) => attr,
            };
            let key = match &attr.name {
                ast::JSXAttrName::Ident(name) => name.sym.to_string(),
                ast::JSXAttrName::JSXNamespacedName(name) => {
                    format!("{}:{}", name.ns.sym, name.name.sym)
                }
            };
            let value = attr
                .value
                .as_ref()
                .map(|v| self.attribute_value(v))
                .unwrap_or_else(|| bool_value(true));
            if key == "ref" {
                let callback = self.ref_value(value);
                result.statements.insert(
                    0,
                    statement(call(callback, vec![ast::Expr::Ident(node.clone())])),
                );
            } else if let Some(directive) = key.strip_prefix("use:") {
                let use_call = self.helper(
                    "use",
                    vec![
                        ast::Expr::Ident(ident(directive)),
                        ast::Expr::Ident(node.clone()),
                        arrow(value),
                    ],
                );
                result.statements.insert(0, statement(use_call));
            } else if has_spread {
                let dynamic = is_dynamic(&value);
                props.push(property(&key, value, dynamic));
            } else if key == "children" {
                // The explicit JSX children win over the children attribute.
                if !has_children {
                    children_prop = Some(if is_dynamic(&value) {
                        arrow(value)
                    } else {
                        value
                    });
                }
            } else if is_dynamic(&value) {
                result.dynamics.push((node.clone(), key, value));
            } else {
                let set = self.helper(
                    "setProp",
                    vec![ast::Expr::Ident(node.clone()), string(&key), value],
                );
                result.statements.push(statement(set));
            }
        }
        if has_spread {
            if !props.is_empty() {
                chunks.push(object(props));
            }
            let props = if chunks.len() == 1 {
                chunks.remove(0)
            } else {
                self.helper("mergeProps", chunks)
            };
            let spread = self.helper(
                "spread",
                vec![
                    ast::Expr::Ident(node.clone()),
                    props,
                    bool_value(has_children),
                ],
            );
            result.statements.push(statement(spread));
        }

        // Allocate and attach static siblings before inserting dynamic ranges.
        // Each insert uses the next static sibling as its marker; null marks the
        // end of a multi-child range, while no marker owns the whole parent.
        let mut children: Vec<(Option<ast::Ident>, Vec<ast::Stmt>, Option<ast::Expr>)> = Vec::new();
        let mut text = String::new();
        let flush_text = |this: &mut Self,
                          text: &mut String,
                          result: &mut NativeElement,
                          children: &mut Vec<_>| {
            if text.is_empty() {
                return;
            }
            let id = this.temporary();
            let create = this.helper("createTextNode", vec![string(text)]);
            result.declarations.push(binding(id.clone(), create));
            children.push((Some(id), Vec::new(), None));
            text.clear();
        };
        for child in &element.children {
            if let Some(value) = text_child(child) {
                text.push_str(&value);
                continue;
            }
            if matches!(child, ast::JSXElementChild::JSXExprContainer(c) if matches!(c.expr, ast::JSXExpr::JSXEmptyExpr(_)))
            {
                continue;
            }
            flush_text(self, &mut text, &mut result, &mut children);
            if let ast::JSXElementChild::JSXElement(element) = child {
                if self.element_name(&element.opening.name).1 {
                    let mut child = self.native_parts(element);
                    result.declarations.append(&mut child.declarations);
                    result.dynamics.append(&mut child.dynamics);
                    children.push((Some(child.node), child.statements, None));
                    continue;
                }
            }
            if let Some(value) = self.child(child, true) {
                children.push((None, Vec::new(), Some(value)));
            }
        }
        flush_text(self, &mut text, &mut result, &mut children);
        if let Some(value) = children_prop {
            children.push((None, Vec::new(), Some(value)));
        }
        let mut appends = Vec::new();
        for (index, (id, stmts, value)) in children.iter().enumerate() {
            if let Some(id) = id {
                appends.push(statement(self.helper(
                    "insertNode",
                    vec![ast::Expr::Ident(node.clone()), ast::Expr::Ident(id.clone())],
                )));
                result.statements.extend(stmts.clone());
            } else if let Some(value) = value {
                let mut args = vec![ast::Expr::Ident(node.clone()), value.clone()];
                if children.len() > 1 {
                    args.push(
                        children[index + 1..]
                            .iter()
                            .find_map(|c| c.0.clone())
                            .map(ast::Expr::Ident)
                            .unwrap_or_else(null),
                    );
                }
                result
                    .statements
                    .push(statement(self.helper("insert", args)));
            }
        }
        appends.append(&mut result.statements);
        result.statements = appends;
        result
    }

    fn property_effect(&mut self, dynamics: Vec<(ast::Ident, String, ast::Expr)>) -> ast::Expr {
        let previous = self.temporary();
        let mut reads = Vec::new();
        let mut writes = Vec::new();
        let mut initial = Vec::new();
        for (index, (node, key, value)) in dynamics.into_iter().enumerate() {
            let current = self.temporary();
            reads.push(binding(current.clone(), value));
            let slot = format!("p{index}");
            let prev = field(&previous, &slot);
            initial.push(property(&slot, ast::Expr::Ident(ident("undefined")), false));
            let set = self.helper(
                "setProp",
                vec![
                    ast::Expr::Ident(node),
                    string(&key),
                    ast::Expr::Ident(current.clone()),
                    prev.clone(),
                ],
            );
            let assign = ast::Expr::Assign(ast::AssignExpr {
                span: DUMMY_SP,
                op: ast::AssignOp::Assign,
                left: ast::AssignTarget::try_from(Box::new(prev.clone())).unwrap(),
                right: Box::new(set),
            });
            writes.push(ast::Stmt::If(ast::IfStmt {
                span: DUMMY_SP,
                test: Box::new(ast::Expr::Bin(ast::BinExpr {
                    span: DUMMY_SP,
                    op: ast::BinaryOp::NotEqEq,
                    left: Box::new(ast::Expr::Ident(current)),
                    right: Box::new(prev),
                })),
                cons: Box::new(statement(assign)),
                alt: None,
            }));
        }
        reads.append(&mut writes);
        reads.push(ast::Stmt::Return(ast::ReturnStmt {
            span: DUMMY_SP,
            arg: Some(Box::new(ast::Expr::Ident(previous.clone()))),
        }));
        let callback = ast::Expr::Arrow(ast::ArrowExpr {
            params: vec![ast::Pat::Ident(previous.into())],
            body: Box::new(ast::BlockStmtOrExpr::BlockStmt(ast::BlockStmt {
                stmts: reads,
                ..Default::default()
            })),
            ..Default::default()
        });
        self.helper("effect", vec![callback, object(initial)])
    }
}
