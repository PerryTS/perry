/// (Issue #50) Return `true` if any statement in `stmts` mutates the local
/// `id`. A local is "mutated" if:
///   - It's the target of a `LocalSet` or `Update` (reassignment), or
///   - A property/index set or update has a root object that resolves to
///     `LocalGet(id)` — covers direct and nested reachable-value writes.
///   - A `NativeMethodCall` targets `LocalGet(id)` with a name from the
///     Array mutating set (`push`, `pop`, `shift`, `unshift`, `splice`,
///     `sort`, `reverse`, `fill`, `copyWithin`).
///
/// Conservative by design: a true positive means we must fall back from
/// the flat-const optimization to the normal arena path. A false positive
/// (flagging something that never actually mutates) only costs us the
/// flat-table win.
pub fn has_any_mutation(stmts: &[perry_hir::Stmt], id: u32) -> bool {
    use perry_hir::Stmt;
    for s in stmts {
        match s {
            Stmt::Expr(e) | Stmt::Throw(e) if expr_has_mutation(e, id) => {
                return true;
            }
            Stmt::Return(Some(e)) if expr_has_mutation(e, id) => {
                return true;
            }
            Stmt::Let { init: Some(e), .. } if expr_has_mutation(e, id) => {
                return true;
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                if expr_has_mutation(condition, id) {
                    return true;
                }
                if has_any_mutation(then_branch, id) {
                    return true;
                }
                if let Some(eb) = else_branch {
                    if has_any_mutation(eb, id) {
                        return true;
                    }
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                if expr_has_mutation(condition, id) {
                    return true;
                }
                if has_any_mutation(body, id) {
                    return true;
                }
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    if has_any_mutation(std::slice::from_ref(init_stmt), id) {
                        return true;
                    }
                }
                if let Some(c) = condition {
                    if expr_has_mutation(c, id) {
                        return true;
                    }
                }
                if let Some(u) = update {
                    if expr_has_mutation(u, id) {
                        return true;
                    }
                }
                if has_any_mutation(body, id) {
                    return true;
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                if has_any_mutation(body, id) {
                    return true;
                }
                if let Some(c) = catch {
                    if has_any_mutation(&c.body, id) {
                        return true;
                    }
                }
                if let Some(f) = finally {
                    if has_any_mutation(f, id) {
                        return true;
                    }
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                if expr_has_mutation(discriminant, id) {
                    return true;
                }
                for c in cases {
                    if let Some(t) = &c.test {
                        if expr_has_mutation(t, id) {
                            return true;
                        }
                    }
                    if has_any_mutation(&c.body, id) {
                        return true;
                    }
                }
            }
            Stmt::Labeled { body, .. }
                if has_any_mutation(std::slice::from_ref(body.as_ref()), id) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

pub fn is_local_get_chain(e: &perry_hir::Expr, id: u32) -> bool {
    use perry_hir::Expr;
    match e {
        Expr::LocalGet(i) => *i == id,
        Expr::IndexGet { object, .. } => is_local_get_chain(object, id),
        Expr::PropertyGet { object, .. } => is_local_get_chain(object, id),
        _ => false,
    }
}

pub fn expr_has_mutation(e: &perry_hir::Expr, id: u32) -> bool {
    use perry_hir::{ArrayElement, CallArg, Expr};
    const ARRAY_MUTATORS: &[&str] = &[
        "push",
        "pop",
        "shift",
        "unshift",
        "splice",
        "sort",
        "reverse",
        "fill",
        "copyWithin",
    ];
    match e {
        Expr::LocalSet(tgt, value) => *tgt == id || expr_has_mutation(value, id),
        Expr::Update { id: tgt, .. } => *tgt == id,
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            is_local_get_chain(object, id)
                || expr_has_mutation(object, id)
                || expr_has_mutation(index, id)
                || expr_has_mutation(value, id)
        }
        Expr::NativeMethodCall {
            object: Some(obj),
            method,
            args,
            ..
        } if ARRAY_MUTATORS.contains(&method.as_str()) && is_local_get_chain(obj, id) => true,
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                if expr_has_mutation(o, id) {
                    return true;
                }
            }
            args.iter().any(|a| expr_has_mutation(a, id))
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            expr_has_mutation(left, id) || expr_has_mutation(right, id)
        }
        Expr::Unary { operand, .. }
        | Expr::Void(operand)
        | Expr::TypeOf(operand)
        | Expr::Await(operand)
        | Expr::Delete(operand)
        | Expr::StringCoerce(operand)
        | Expr::ObjectCoerce(operand)
        | Expr::BooleanCoerce(operand)
        | Expr::NumberCoerce(operand) => expr_has_mutation(operand, id),
        Expr::Call { callee, args, .. } => {
            if expr_has_mutation(callee, id) {
                return true;
            }
            args.iter().any(|a| expr_has_mutation(a, id))
        }
        Expr::CallSpread { callee, args, .. } => {
            if expr_has_mutation(callee, id) {
                return true;
            }
            args.iter().any(|a| match a {
                CallArg::Expr(e) | CallArg::Spread(e) => expr_has_mutation(e, id),
            })
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            expr_has_mutation(condition, id)
                || expr_has_mutation(then_expr, id)
                || expr_has_mutation(else_expr, id)
        }
        Expr::PropertyGet { object, .. } => expr_has_mutation(object, id),
        Expr::PropertySet { object, value, .. } => {
            is_local_get_chain(object, id)
                || expr_has_mutation(object, id)
                || expr_has_mutation(value, id)
        }
        Expr::PropertyUpdate { object, .. } => {
            is_local_get_chain(object, id) || expr_has_mutation(object, id)
        }
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            ..
        } => {
            is_local_get_chain(target, id)
                || is_local_get_chain(receiver, id)
                || expr_has_mutation(target, id)
                || expr_has_mutation(key, id)
                || expr_has_mutation(value, id)
                || expr_has_mutation(receiver, id)
        }
        Expr::IndexGet { object, index } => {
            expr_has_mutation(object, id) || expr_has_mutation(index, id)
        }
        Expr::Array(elements) => elements.iter().any(|e| expr_has_mutation(e, id)),
        Expr::ArraySpread(elements) => elements.iter().any(|el| match el {
            ArrayElement::Expr(e) | ArrayElement::Spread(e) => expr_has_mutation(e, id),
            ArrayElement::Hole => false,
        }),
        Expr::Object(props) => props.iter().any(|(_, v)| expr_has_mutation(v, id)),
        Expr::Closure { body, .. } => has_any_mutation(body, id),
        Expr::Sequence(es) => es.iter().any(|e| expr_has_mutation(e, id)),
        Expr::ArrayPush { array_id, value } => *array_id == id || expr_has_mutation(value, id),
        Expr::ArraySplice {
            array_id,
            start,
            delete_count,
            items,
        } => {
            *array_id == id
                || expr_has_mutation(start, id)
                || delete_count
                    .as_ref()
                    .is_some_and(|d| expr_has_mutation(d, id))
                || items.iter().any(|it| expr_has_mutation(it, id))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::{BinaryOp, Expr};

    #[test]
    fn direct_property_writes_mutate_the_root_binding_value() {
        let set = Expr::PropertySet {
            object: Box::new(Expr::LocalGet(7)),
            property: "count".to_string(),
            value: Box::new(Expr::String("lie".to_string())),
        };
        let update = Expr::PropertyUpdate {
            object: Box::new(Expr::LocalGet(7)),
            property: "count".to_string(),
            op: BinaryOp::Add,
            prefix: false,
        };
        assert!(expr_has_mutation(&set, 7));
        assert!(expr_has_mutation(&update, 7));
        assert!(!expr_has_mutation(&set, 8));
    }

    #[test]
    fn lowered_put_value_write_mutates_target_and_receiver_roots() {
        let set = Expr::PutValueSet {
            target: Box::new(Expr::LocalGet(7)),
            key: Box::new(Expr::String("count".to_string())),
            value: Box::new(Expr::String("changed".to_string())),
            receiver: Box::new(Expr::LocalGet(8)),
            strict: true,
        };
        assert!(expr_has_mutation(&set, 7));
        assert!(expr_has_mutation(&set, 8));
        assert!(!expr_has_mutation(&set, 9));
    }
}
