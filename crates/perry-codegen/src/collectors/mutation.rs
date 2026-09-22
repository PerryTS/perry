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
    any_top_level_expr(stmts, &mut |e| expr_has_mutation(e, id))
}

/// Return `true` when every use of a candidate flat constant 2D array stays
/// within the representation that codegen can substitute safely.
///
/// The flat table is a read-only copy of the JavaScript array.  It may replace
/// `array[row][column]` and `const row = array[index]; row[column]`, but the
/// real row object must remain observable for every other use.  In particular,
/// returning a row, passing it to a call, comparing it by identity, or writing
/// through it would let the heap array and flat table disagree.
pub fn flat_const_array_uses_are_read_only(stmts: &[perry_hir::Stmt], id: u32) -> bool {
    let mut row_aliases = std::collections::HashSet::new();
    collect_flat_const_row_aliases(stmts, id, &mut row_aliases);
    flat_const_stmts_are_safe(stmts, id, &row_aliases)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FlatConstUse {
    Value,
    IndexObject,
    RowAliasInitializer,
}

fn collect_flat_const_row_aliases(
    stmts: &[perry_hir::Stmt],
    id: u32,
    out: &mut std::collections::HashSet<u32>,
) {
    use perry_hir::{Expr, Stmt};
    for stmt in stmts {
        match stmt {
            Stmt::Let {
                id: alias_id,
                init: Some(Expr::IndexGet { object, .. }),
                mutable: false,
                ..
            } if matches!(object.as_ref(), Expr::LocalGet(array_id) if *array_id == id) => {
                out.insert(*alias_id);
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_flat_const_row_aliases(then_branch, id, out);
                if let Some(else_branch) = else_branch {
                    collect_flat_const_row_aliases(else_branch, id, out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_flat_const_row_aliases(body, id, out);
            }
            Stmt::For { init, body, .. } => {
                if let Some(init) = init {
                    collect_flat_const_row_aliases(std::slice::from_ref(init.as_ref()), id, out);
                }
                collect_flat_const_row_aliases(body, id, out);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_flat_const_row_aliases(body, id, out);
                if let Some(catch) = catch {
                    collect_flat_const_row_aliases(&catch.body, id, out);
                }
                if let Some(finally) = finally {
                    collect_flat_const_row_aliases(finally, id, out);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_flat_const_row_aliases(&case.body, id, out);
                }
            }
            Stmt::Labeled { body, .. } => {
                collect_flat_const_row_aliases(std::slice::from_ref(body.as_ref()), id, out);
            }
            _ => {}
        }
    }
}

fn flat_const_stmts_are_safe(
    stmts: &[perry_hir::Stmt],
    id: u32,
    row_aliases: &std::collections::HashSet<u32>,
) -> bool {
    use perry_hir::Stmt;
    for stmt in stmts {
        let safe = match stmt {
            Stmt::Let {
                id: alias_id,
                init: Some(init),
                ..
            } => {
                let use_kind = if row_aliases.contains(alias_id) {
                    FlatConstUse::RowAliasInitializer
                } else {
                    FlatConstUse::Value
                };
                flat_const_expr_is_safe(init, id, row_aliases, use_kind)
            }
            Stmt::Expr(expr) | Stmt::Throw(expr) => {
                flat_const_expr_is_safe(expr, id, row_aliases, FlatConstUse::Value)
            }
            Stmt::Return(Some(expr)) => {
                flat_const_expr_is_safe(expr, id, row_aliases, FlatConstUse::Value)
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                flat_const_expr_is_safe(condition, id, row_aliases, FlatConstUse::Value)
                    && flat_const_stmts_are_safe(then_branch, id, row_aliases)
                    && else_branch
                        .as_ref()
                        .is_none_or(|branch| flat_const_stmts_are_safe(branch, id, row_aliases))
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                flat_const_expr_is_safe(condition, id, row_aliases, FlatConstUse::Value)
                    && flat_const_stmts_are_safe(body, id, row_aliases)
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().is_none_or(|init| {
                    flat_const_stmts_are_safe(std::slice::from_ref(init.as_ref()), id, row_aliases)
                }) && condition.as_ref().is_none_or(|expr| {
                    flat_const_expr_is_safe(expr, id, row_aliases, FlatConstUse::Value)
                }) && update.as_ref().is_none_or(|expr| {
                    flat_const_expr_is_safe(expr, id, row_aliases, FlatConstUse::Value)
                }) && flat_const_stmts_are_safe(body, id, row_aliases)
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                flat_const_stmts_are_safe(body, id, row_aliases)
                    && catch
                        .as_ref()
                        .is_none_or(|catch| flat_const_stmts_are_safe(&catch.body, id, row_aliases))
                    && finally
                        .as_ref()
                        .is_none_or(|finally| flat_const_stmts_are_safe(finally, id, row_aliases))
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                flat_const_expr_is_safe(discriminant, id, row_aliases, FlatConstUse::Value)
                    && cases.iter().all(|case| {
                        case.test.as_ref().is_none_or(|test| {
                            flat_const_expr_is_safe(test, id, row_aliases, FlatConstUse::Value)
                        }) && flat_const_stmts_are_safe(&case.body, id, row_aliases)
                    })
            }
            Stmt::Labeled { body, .. } => {
                flat_const_stmts_are_safe(std::slice::from_ref(body.as_ref()), id, row_aliases)
            }
            _ => true,
        };
        if !safe {
            return false;
        }
    }
    true
}

fn flat_const_expr_is_safe(
    expr: &perry_hir::Expr,
    id: u32,
    row_aliases: &std::collections::HashSet<u32>,
    use_kind: FlatConstUse,
) -> bool {
    use perry_hir::Expr;
    match expr {
        Expr::LocalGet(local_id) if *local_id == id || row_aliases.contains(local_id) => {
            use_kind == FlatConstUse::IndexObject
        }
        Expr::IndexGet { object, index } => {
            let produces_row =
                matches!(object.as_ref(), Expr::LocalGet(array_id) if *array_id == id);
            if produces_row
                && !matches!(
                    use_kind,
                    FlatConstUse::IndexObject | FlatConstUse::RowAliasInitializer
                )
            {
                return false;
            }
            flat_const_expr_is_safe(object, id, row_aliases, FlatConstUse::IndexObject)
                && flat_const_expr_is_safe(index, id, row_aliases, FlatConstUse::Value)
        }
        Expr::Closure { body, .. } => flat_const_array_uses_are_read_only(body, id),
        _ => {
            let mut safe = true;
            perry_hir::walker::walk_expr_children(expr, &mut |child| {
                if safe && !flat_const_expr_is_safe(child, id, row_aliases, FlatConstUse::Value) {
                    safe = false;
                }
            });
            safe
        }
    }
}

/// (#8094) Does any call reach unknown code anywhere in `stmts`?
///
/// A guarded parameter's descriptor is validated once, at entry. It describes
/// a heap object, and unknown code can reach that object WITHOUT us handing it
/// over: the caller may already have stored it in a global, captured it in a
/// closure, or hung it off another live object before calling us. So an
/// "escape analysis" over our own argument lists is not sufficient — measured,
/// see the `poison()` case in `test_gap_specabi_ordinary_param_guards.ts`,
/// where the parameter is never passed anywhere and is still mutated. The
/// sound question is therefore "did unknown code run", not "did the reference
/// escape".
///
/// Shares `any_top_level_expr` with `has_any_mutation` so the two can never
/// drift apart on statement coverage.
pub fn body_contains_call(stmts: &[perry_hir::Stmt]) -> bool {
    any_top_level_expr(stmts, &mut expr_contains_call)
}

/// #8648: does this body create a closure anywhere?
///
/// A derived constructor's `super()` binding normally lives in a plain `i1`
/// alloca that `bind_derived_this_after_super` reads directly. The RUNTIME
/// form (`js_derived_super_scope_push`/`pop`, a thread-local stack) exists for
/// exactly one reason, stated on `push_shared_super_called_slot`: an arrow
/// inside the constructor compiles as its OWN LLVM function, so it cannot name
/// the outer alloca and has to find the cell through
/// `js_derived_super_bind_current` / `js_derived_this_check_current`.
///
/// A derived constructor with no closure in it therefore pays a thread-local
/// push and pop per construction for a cell nothing else can look up. #8630
/// emitted the shared form for every derived constructor, which cost 1.89x on
/// a two-class `new B(x, y)` loop and 3.14x on `shapes.ts` (six `extends`,
/// 120k constructions).
///
/// Deliberately conservative: ANY closure in the body selects the shared form,
/// without asking whether that closure actually mentions `this` or `super`.
/// The cheap answer is only taken when there is provably no separate LLVM
/// function to serve.
pub fn body_contains_closure(stmts: &[perry_hir::Stmt]) -> bool {
    any_top_level_expr(stmts, &mut expr_contains_closure)
}

fn expr_contains_closure(expr: &perry_hir::Expr) -> bool {
    if matches!(expr, perry_hir::Expr::Closure { .. }) {
        return true;
    }
    let mut found = false;
    perry_hir::walker::walk_expr_children(expr, &mut |child: &perry_hir::Expr| {
        if !found && expr_contains_closure(child) {
            found = true;
        }
    });
    found
}

/// The statement skeleton both predicates walk. `pred` is applied to each
/// top-level expression; it is responsible for its own subexpression
/// recursion.
fn any_top_level_expr(
    stmts: &[perry_hir::Stmt],
    pred: &mut impl FnMut(&perry_hir::Expr) -> bool,
) -> bool {
    use perry_hir::Stmt;
    for s in stmts {
        match s {
            Stmt::Expr(e) | Stmt::Throw(e) if pred(e) => {
                return true;
            }
            Stmt::Return(Some(e)) if pred(e) => {
                return true;
            }
            Stmt::Let { init: Some(e), .. } if pred(e) => {
                return true;
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                if pred(condition) {
                    return true;
                }
                if any_top_level_expr(then_branch, pred) {
                    return true;
                }
                if let Some(eb) = else_branch {
                    if any_top_level_expr(eb, pred) {
                        return true;
                    }
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                if pred(condition) {
                    return true;
                }
                if any_top_level_expr(body, pred) {
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
                    if any_top_level_expr(std::slice::from_ref(init_stmt), pred) {
                        return true;
                    }
                }
                if let Some(c) = condition {
                    if pred(c) {
                        return true;
                    }
                }
                if let Some(u) = update {
                    if pred(u) {
                        return true;
                    }
                }
                if any_top_level_expr(body, pred) {
                    return true;
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                if any_top_level_expr(body, pred) {
                    return true;
                }
                if let Some(c) = catch {
                    if any_top_level_expr(&c.body, pred) {
                        return true;
                    }
                }
                if let Some(f) = finally {
                    if any_top_level_expr(f, pred) {
                        return true;
                    }
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                if pred(discriminant) {
                    return true;
                }
                for c in cases {
                    if let Some(t) = &c.test {
                        if pred(t) {
                            return true;
                        }
                    }
                    if any_top_level_expr(&c.body, pred) {
                        return true;
                    }
                }
            }
            Stmt::Labeled { body, .. }
                if any_top_level_expr(std::slice::from_ref(body.as_ref()), pred) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

/// (#8094) Can evaluating this expression, or any subexpression, transfer
/// control to code this analysis cannot see?
///
/// The match lists the variants that are provably call-free and lets
/// EVERYTHING ELSE fall to `_ => true`. The default direction is deliberate:
/// `Expr` has >550 variants, so an exhaustive match is not maintainable, and
/// a new variant defaulting to "cannot call" would silently re-open a
/// wrong-code bug. Defaulting to "may call" only costs a missed
/// optimization.
///
/// KNOWN RESIDUAL, deliberately not covered: a property read, an index read
/// or an arithmetic coercion can run an accessor, a Proxy trap or a
/// `valueOf`/`toString`, and that code could in principle reach a guarded
/// object through a pre-existing alias. Those are listed as call-free here
/// because treating every field read as unknown code makes the whole
/// specialization vacuous. The guarded object itself cannot carry an accessor
/// — `own_data_field` in the runtime descriptor admits plain data properties
/// only — so this needs a SECOND object whose accessor reaches the first.
/// Tracked separately; the demonstrated bug class (explicit calls) is closed.
fn expr_contains_call(e: &perry_hir::Expr) -> bool {
    use perry_hir::Expr;
    let here = !matches!(
        e,
        Expr::Undefined
            | Expr::Null
            | Expr::Bool(_)
            | Expr::Number(_)
            | Expr::Integer(_)
            | Expr::BigInt(_)
            | Expr::String(_)
            | Expr::WtfString(_)
            | Expr::LocalGet(_)
            | Expr::LocalSet(..)
            | Expr::GlobalGet(_)
            | Expr::GlobalSet(..)
            | Expr::Update { .. }
            | Expr::Logical { .. }
            | Expr::Conditional { .. }
            | Expr::TypeOf(_)
            | Expr::Void(_)
            | Expr::FuncRef(_)
            | Expr::Object(_)
            | Expr::Array(_)
            | Expr::NewTarget
            | Expr::ClassRef(_)
            | Expr::EnumMember { .. }
            | Expr::PrivateBrandCheck { .. }
            | Expr::Binary { .. }
            | Expr::Compare { .. }
            | Expr::Unary { .. }
            | Expr::PropertyGet { .. }
            | Expr::IndexGet { .. }
            | Expr::PropertySet { .. }
            | Expr::IndexSet { .. }
            | Expr::PropertyUpdate { .. }
            | Expr::IndexUpdate { .. }
    );
    if here {
        return true;
    }
    let mut found = false;
    perry_hir::walker::walk_expr_children(e, &mut |child| {
        if !found && expr_contains_call(child) {
            found = true;
        }
    });
    found
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
        | Expr::TemplateStringCoerce(operand)
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
        Expr::ArrayPush {
            array_id, value, ..
        } => *array_id == id || expr_has_mutation(value, id),
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

    fn index(object: Expr, index: i64) -> Expr {
        Expr::IndexGet {
            object: Box::new(object),
            index: Box::new(Expr::Integer(index)),
        }
    }

    fn row_alias(id: u32, array_id: u32) -> perry_hir::Stmt {
        perry_hir::Stmt::Let {
            id,
            name: "row".to_string(),
            ty: perry_hir::types::Type::Any,
            mutable: false,
            init: Some(index(Expr::LocalGet(array_id), 0)),
        }
    }

    #[test]
    fn flat_const_allows_direct_and_aliased_element_reads() {
        let body = vec![
            row_alias(8, 7),
            perry_hir::Stmt::Expr(index(index(Expr::LocalGet(7), 0), 1)),
            perry_hir::Stmt::Return(Some(index(Expr::LocalGet(8), 2))),
        ];
        assert!(flat_const_array_uses_are_read_only(&body, 7));
    }

    #[test]
    fn flat_const_rejects_rows_used_as_javascript_values() {
        let returned_directly = vec![perry_hir::Stmt::Return(Some(index(Expr::LocalGet(7), 0)))];
        assert!(!flat_const_array_uses_are_read_only(&returned_directly, 7));

        let returned_alias = vec![
            row_alias(8, 7),
            perry_hir::Stmt::Return(Some(Expr::LocalGet(8))),
        ];
        assert!(!flat_const_array_uses_are_read_only(&returned_alias, 7));
    }

    #[test]
    fn flat_const_rejects_nested_element_writes() {
        let write = Expr::IndexSet {
            object: Box::new(index(Expr::LocalGet(7), 0)),
            index: Box::new(Expr::Integer(1)),
            value: Box::new(Expr::Integer(99)),
        };
        assert!(!flat_const_array_uses_are_read_only(
            &[perry_hir::Stmt::Expr(write)],
            7
        ));
    }

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
            strict: true,
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

    fn call(callee: u32, args: Vec<Expr>) -> Expr {
        Expr::Call {
            callee: Box::new(Expr::FuncRef(callee)),
            args,
            type_args: Vec::new(),
            byte_offset: 0,
        }
    }

    /// #8094. A reference parameter's entry proof cannot outlive a call, so
    /// the eligibility question is "did unknown code run", not "did the
    /// reference escape". This is the case an escape analysis gets wrong: the
    /// binding is never passed anywhere, and the callee still reaches it
    /// through an alias the caller arranged.
    #[test]
    fn a_call_that_receives_nothing_still_counts_as_unknown_code() {
        let body = vec![
            perry_hir::Stmt::Expr(call(101, Vec::new())),
            perry_hir::Stmt::Return(Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(7)),
                property: "v".to_string(),
                byte_offset: 0,
            })),
        ];
        assert!(body_contains_call(&body));
        // and the escape-shaped predicate is exactly what does NOT see it
        assert!(!has_any_mutation(&body, 7));
    }

    #[test]
    fn a_call_free_body_of_reads_and_arithmetic_is_not_a_call() {
        let read = Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(7)),
            property: "v".to_string(),
            byte_offset: 0,
        };
        let body = vec![perry_hir::Stmt::Return(Some(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(read),
            right: Box::new(Expr::Integer(1)),
        }))];
        assert!(!body_contains_call(&body));
    }

    /// The call may be buried anywhere the shared statement skeleton walks.
    #[test]
    fn calls_are_found_through_nested_statements_and_subexpressions() {
        let nested = perry_hir::Stmt::While {
            condition: Expr::Bool(true),
            body: vec![perry_hir::Stmt::Let {
                id: 3,
                name: "x".to_string(),
                ty: perry_hir::types::Type::Number,
                mutable: false,
                init: Some(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::Integer(1)),
                    right: Box::new(call(102, vec![Expr::Integer(2)])),
                }),
            }],
        };
        assert!(body_contains_call(&[nested]));
    }
}
