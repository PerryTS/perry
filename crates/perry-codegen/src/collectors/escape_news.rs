use perry_hir::{BinaryOp, Expr, Function, Stmt};
use std::collections::HashSet;

use super::*;

pub fn collect_non_escaping_news(
    stmts: &[perry_hir::Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &std::collections::HashMap<u32, String>,
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
) -> std::collections::HashMap<u32, String> {
    // Pass 1: find candidates — Let bindings of New that aren't boxed/global.
    let mut candidates: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    find_new_candidates(stmts, boxed_vars, module_globals, &mut candidates);

    if candidates.is_empty() {
        return candidates;
    }

    // Pass 2: walk all stmts/exprs checking every use of each candidate.
    // Any unsafe use marks the id as escaped.
    let mut escaped: HashSet<u32> = HashSet::new();
    check_escapes_in_stmts(stmts, &candidates, classes, &mut escaped);

    // Pass 3 (issue #313): if the candidate's class constructor or any
    // instance-field initializer materializes `this` as a value, scalar
    // replacement cannot soundly inline it — `Expr::This` reads from the
    // dummy `this_stack` slot allocated at stmt.rs:316, which is never
    // populated (there is no real heap `this` in scalar replacement). Mark
    // such candidates as escaped so they take the heap-allocated path.
    for (id, class_name) in &candidates {
        if escaped.contains(id) {
            continue;
        }
        if let Some(class) = classes.get(class_name) {
            if class_uses_this_as_value(class, classes) {
                escaped.insert(*id);
            }
            // Issue #573: classes extending built-in Error / TypeError /
            // etc. need the heap path so `lower_new`'s Error-init fallback
            // can populate `this.message` / `this.name` via
            // `js_object_set_field_by_name`. Scalar replacement allocates
            // per-field allocas keyed by declared field names, but Error
            // subclasses typically declare neither field — the runtime
            // adds them via SuperCall / lower_new fallback. Without this
            // check, `class MyError extends Error {}` skips the heap path
            // and the scalar-replaced object has no slots for `message` /
            // `name`, so reads return undefined or crash.
            else if class_chain_extends_builtin_error(class, classes) {
                escaped.insert(*id);
            }
        }
    }

    candidates.retain(|id, _| !escaped.contains(id));
    candidates
}

/// Issue #313: detect class constructor / field-initializer patterns that
/// materialize `this` as a value (i.e. read it as a NaN-boxed heap pointer
/// rather than just dereferencing fields off it). Scalar replacement of
/// `let h = new C(...)` inlines the ctor body with a dummy `this_stack` slot
/// — `this.field = …` and `this.field` are intercepted in expr.rs and routed
/// to the per-field allocas, but anything else that touches `this` itself
/// reads the uninitialized dummy and silently produces TAG_UNDEFINED.
///
/// Unsafe patterns (return `true`):
///   - `Expr::This` outside of `(PropertyGet|PropertySet|PropertyUpdate).object`
///     with a *field* property (e.g. `const self = this`, `someFn(this)`,
///     `return this`).
///   - `PropertyGet/Set/Update { object: This, property }` where `property`
///     is NOT an instance field of the class — i.e. method/getter calls,
///     since the dispatcher passes `this` as `recv_box` to the callee.
///   - `Expr::Closure { captures_this: true, .. }` — the closure env stores
///     `this` at the construction site.
///   - `Expr::SuperCall` / `Expr::SuperMethodCall` — `super(...)` and
///     `super.foo(...)` need the real `this`.
pub fn class_uses_this_as_value(
    class: &perry_hir::Class,
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
) -> bool {
    // Collect all instance fields from this class + parent chain so the
    // "is this.X a field?" check honors inheritance.
    let mut field_names: HashSet<String> = HashSet::new();
    field_names.extend(class.fields.iter().map(|f| f.name.clone()));
    let mut parent = class.extends_name.as_deref();
    while let Some(p) = parent {
        if let Some(pc) = classes.get(p) {
            field_names.extend(pc.fields.iter().map(|f| f.name.clone()));
            parent = pc.extends_name.as_deref();
        } else {
            break;
        }
    }
    if let Some(ctor) = &class.constructor {
        if stmts_use_this_as_value(&ctor.body, &field_names) {
            return true;
        }
    }
    for f in &class.fields {
        if let Some(init) = &f.init {
            if expr_uses_this_as_value(init, &field_names) {
                return true;
            }
        }
    }
    // Parent fields are initialized via apply_field_initializers_recursive
    // in scalar replacement; check their initializers too.
    let mut parent = class.extends_name.as_deref();
    while let Some(p) = parent {
        if let Some(pc) = classes.get(p) {
            for f in &pc.fields {
                if let Some(init) = &f.init {
                    if expr_uses_this_as_value(init, &field_names) {
                        return true;
                    }
                }
            }
            parent = pc.extends_name.as_deref();
        } else {
            break;
        }
    }
    false
}

/// Issue #573: walk the class's `extends_name` chain and return true if any
/// ancestor name matches a built-in Error subclass — `Error`, `TypeError`,
/// `RangeError`, etc. Such classes need real heap allocation so
/// `lower_new`'s Error-init fallback (and the user-explicit `super(msg)`
/// path) can populate `this.message` / `this.name` via the runtime field-
/// setter. Scalar replacement only allocates allocas for declared fields,
/// which Error subclasses typically don't declare.
pub fn class_chain_extends_builtin_error(
    class: &perry_hir::Class,
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
) -> bool {
    let mut cur = class.extends_name.as_deref().map(|s| s.to_string());
    let mut depth = 0usize;
    while let Some(name) = cur {
        if matches!(
            name.as_str(),
            "Error"
                | "TypeError"
                | "RangeError"
                | "ReferenceError"
                | "SyntaxError"
                | "URIError"
                | "EvalError"
                | "AggregateError"
        ) {
            return true;
        }
        cur = classes
            .get(name.as_str())
            .and_then(|c| c.extends_name.clone());
        depth += 1;
        if depth > 32 {
            break;
        }
    }
    false
}

pub fn stmts_use_this_as_value(stmts: &[perry_hir::Stmt], fields: &HashSet<String>) -> bool {
    use perry_hir::Stmt;
    for s in stmts {
        let bad = match s {
            Stmt::Expr(e) | Stmt::Throw(e) => expr_uses_this_as_value(e, fields),
            Stmt::Return(opt) => opt
                .as_ref()
                .is_some_and(|e| expr_uses_this_as_value(e, fields)),
            Stmt::Let { init, .. } => init
                .as_ref()
                .is_some_and(|e| expr_uses_this_as_value(e, fields)),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                expr_uses_this_as_value(condition, fields)
                    || stmts_use_this_as_value(then_branch, fields)
                    || else_branch
                        .as_ref()
                        .is_some_and(|eb| stmts_use_this_as_value(eb, fields))
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                expr_uses_this_as_value(condition, fields) || stmts_use_this_as_value(body, fields)
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().is_some_and(|i| {
                    stmts_use_this_as_value(std::slice::from_ref(i.as_ref()), fields)
                }) || condition
                    .as_ref()
                    .is_some_and(|c| expr_uses_this_as_value(c, fields))
                    || update
                        .as_ref()
                        .is_some_and(|u| expr_uses_this_as_value(u, fields))
                    || stmts_use_this_as_value(body, fields)
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                stmts_use_this_as_value(body, fields)
                    || catch
                        .as_ref()
                        .is_some_and(|c| stmts_use_this_as_value(&c.body, fields))
                    || finally
                        .as_ref()
                        .is_some_and(|f| stmts_use_this_as_value(f, fields))
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                expr_uses_this_as_value(discriminant, fields)
                    || cases.iter().any(|c| {
                        c.test
                            .as_ref()
                            .is_some_and(|t| expr_uses_this_as_value(t, fields))
                            || stmts_use_this_as_value(&c.body, fields)
                    })
            }
            Stmt::Labeled { body, .. } => {
                stmts_use_this_as_value(std::slice::from_ref(body.as_ref()), fields)
            }
            Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {
                false
            }
            Stmt::PreallocateBoxes(_) => false,
        };
        if bad {
            return true;
        }
    }
    false
}

pub fn expr_uses_this_as_value(e: &perry_hir::Expr, fields: &HashSet<String>) -> bool {
    use perry_hir::{ArrayElement, CallArg, Expr};
    match e {
        Expr::This => true,
        Expr::Closure {
            captures_this: true,
            ..
        } => true,
        Expr::SuperCall(_) | Expr::SuperMethodCall { .. } => true,
        // PropertyGet/Set/Update with `this.<field>` is the safe pattern —
        // scalar replacement intercepts it. With `this.<method>` it falls
        // through to the heap-dispatch path which materializes `this`.
        Expr::PropertyGet { object, property } => {
            if matches!(object.as_ref(), Expr::This) {
                return !fields.contains(property);
            }
            expr_uses_this_as_value(object, fields)
        }
        Expr::PropertySet {
            object,
            value,
            property,
        } => {
            let obj_unsafe = if matches!(object.as_ref(), Expr::This) {
                !fields.contains(property)
            } else {
                expr_uses_this_as_value(object, fields)
            };
            obj_unsafe || expr_uses_this_as_value(value, fields)
        }
        Expr::PropertyUpdate {
            object, property, ..
        } => {
            if matches!(object.as_ref(), Expr::This) {
                return !fields.contains(property);
            }
            expr_uses_this_as_value(object, fields)
        }
        // Closures that don't capture `this` have their own `this` scope —
        // any `Expr::This` inside their body refers to a different binding.
        Expr::Closure {
            captures_this: false,
            ..
        } => false,
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            expr_uses_this_as_value(left, fields) || expr_uses_this_as_value(right, fields)
        }
        Expr::Unary { operand, .. }
        | Expr::Void(operand)
        | Expr::TypeOf(operand)
        | Expr::Await(operand)
        | Expr::Delete(operand)
        | Expr::StringCoerce(operand)
        | Expr::BooleanCoerce(operand)
        | Expr::NumberCoerce(operand) => expr_uses_this_as_value(operand, fields),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            expr_uses_this_as_value(condition, fields)
                || expr_uses_this_as_value(then_expr, fields)
                || expr_uses_this_as_value(else_expr, fields)
        }
        Expr::Call { callee, args, .. } => {
            expr_uses_this_as_value(callee, fields)
                || args.iter().any(|a| expr_uses_this_as_value(a, fields))
        }
        Expr::CallSpread { callee, args, .. } => {
            expr_uses_this_as_value(callee, fields)
                || args.iter().any(|a| match a {
                    CallArg::Expr(e) | CallArg::Spread(e) => expr_uses_this_as_value(e, fields),
                })
        }
        Expr::NativeMethodCall { object, args, .. } => {
            object
                .as_ref()
                .is_some_and(|o| expr_uses_this_as_value(o, fields))
                || args.iter().any(|a| expr_uses_this_as_value(a, fields))
        }
        Expr::IndexGet { object, index } => {
            expr_uses_this_as_value(object, fields) || expr_uses_this_as_value(index, fields)
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            expr_uses_this_as_value(object, fields)
                || expr_uses_this_as_value(index, fields)
                || expr_uses_this_as_value(value, fields)
        }
        Expr::IndexUpdate { object, index, .. } => {
            expr_uses_this_as_value(object, fields) || expr_uses_this_as_value(index, fields)
        }
        Expr::Array(elements) => elements.iter().any(|e| expr_uses_this_as_value(e, fields)),
        Expr::ArraySpread(elements) => elements.iter().any(|el| match el {
            ArrayElement::Expr(e) | ArrayElement::Spread(e) => expr_uses_this_as_value(e, fields),
        }),
        Expr::Object(props) => props
            .iter()
            .any(|(_, v)| expr_uses_this_as_value(v, fields)),
        Expr::ObjectSpread { parts } => parts
            .iter()
            .any(|(_, e)| expr_uses_this_as_value(e, fields)),
        Expr::New { args, .. } => args.iter().any(|a| expr_uses_this_as_value(a, fields)),
        Expr::NewDynamic { callee, args } => {
            expr_uses_this_as_value(callee, fields)
                || args.iter().any(|a| expr_uses_this_as_value(a, fields))
        }
        Expr::LocalSet(_, value) => expr_uses_this_as_value(value, fields),
        Expr::Sequence(es) => es.iter().any(|e| expr_uses_this_as_value(e, fields)),
        Expr::Yield { value, .. } => value
            .as_ref()
            .is_some_and(|v| expr_uses_this_as_value(v, fields)),
        Expr::InstanceOf { expr, .. } => expr_uses_this_as_value(expr, fields),
        Expr::In { property, object } => {
            expr_uses_this_as_value(property, fields) || expr_uses_this_as_value(object, fields)
        }
        // Leaves: don't contain `this`.
        Expr::Integer(_)
        | Expr::Number(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::Undefined
        | Expr::Null
        | Expr::LocalGet(_)
        | Expr::GlobalGet(_)
        | Expr::FuncRef(_)
        | Expr::ClassRef(_)
        | Expr::ExternFuncRef { .. }
        | Expr::EnumMember { .. }
        | Expr::StaticFieldGet { .. }
        | Expr::Update { .. } => false,
        // Catch-all: be conservative — assume the variant might materialize
        // `this`. Disabling scalar replacement is always safe; the cost is
        // missing the optimization on whatever pattern this turns out to be.
        _ => true,
    }
}

/// Is `property` a getter on `class_name` (walking its inheritance chain)?
/// Used by escape analysis: a `LocalGet(candidate).gettableProp` access is
/// a real getter dispatch that needs `this` as a heap pointer, so the
/// candidate must escape.
pub fn is_class_getter(
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
    class_name: &str,
    property: &str,
) -> bool {
    let mut cur = Some(class_name.to_string());
    while let Some(name) = cur {
        if let Some(class) = classes.get(&name) {
            if class.getters.iter().any(|(n, _)| n == property) {
                return true;
            }
            cur = class.extends_name.clone();
        } else {
            return false;
        }
    }
    false
}

/// Mirror of `is_class_getter` for setters — used on the PropertySet/
/// PropertyUpdate paths where a setter dispatch (vs. a plain field write)
/// likewise needs a real `this` pointer.
pub fn is_class_setter(
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
    class_name: &str,
    property: &str,
) -> bool {
    let mut cur = Some(class_name.to_string());
    while let Some(name) = cur {
        if let Some(class) = classes.get(&name) {
            if class.setters.iter().any(|(n, _)| n == property) {
                return true;
            }
            cur = class.extends_name.clone();
        } else {
            return false;
        }
    }
    false
}

/// Pass 1: walk Stmt tree, find `Let { id, init: New { class_name } }`
/// where id is not boxed/global.
pub fn find_new_candidates(
    stmts: &[perry_hir::Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &std::collections::HashMap<u32, String>,
    candidates: &mut std::collections::HashMap<u32, String>,
) {
    use perry_hir::{Expr, Stmt};
    for s in stmts {
        match s {
            Stmt::Let {
                id,
                init: Some(Expr::New { class_name, .. }),
                ..
            } => {
                if !boxed_vars.contains(id) && !module_globals.contains_key(id) {
                    candidates.insert(*id, class_name.clone());
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                find_new_candidates(then_branch, boxed_vars, module_globals, candidates);
                if let Some(eb) = else_branch {
                    find_new_candidates(eb, boxed_vars, module_globals, candidates);
                }
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    find_new_candidates(
                        std::slice::from_ref(init_stmt),
                        boxed_vars,
                        module_globals,
                        candidates,
                    );
                }
                find_new_candidates(body, boxed_vars, module_globals, candidates);
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                find_new_candidates(body, boxed_vars, module_globals, candidates);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                find_new_candidates(body, boxed_vars, module_globals, candidates);
                if let Some(c) = catch {
                    find_new_candidates(&c.body, boxed_vars, module_globals, candidates);
                }
                if let Some(f) = finally {
                    find_new_candidates(f, boxed_vars, module_globals, candidates);
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    find_new_candidates(&c.body, boxed_vars, module_globals, candidates);
                }
            }
            Stmt::Labeled { body, .. } => {
                find_new_candidates(
                    std::slice::from_ref(body.as_ref()),
                    boxed_vars,
                    module_globals,
                    candidates,
                );
            }
            _ => {}
        }
    }
}

/// Pass 2: walk all stmts/exprs checking every use of each candidate.
pub fn check_escapes_in_stmts(
    stmts: &[perry_hir::Stmt],
    candidates: &std::collections::HashMap<u32, String>,
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
    escaped: &mut HashSet<u32>,
) {
    use perry_hir::Stmt;
    for s in stmts {
        match s {
            Stmt::Expr(e) | Stmt::Throw(e) => {
                check_escapes_in_expr(e, candidates, classes, escaped)
            }
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    check_escapes_in_expr(e, candidates, classes, escaped);
                }
            }
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    check_escapes_in_expr(e, candidates, classes, escaped);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                check_escapes_in_expr(condition, candidates, classes, escaped);
                check_escapes_in_stmts(then_branch, candidates, classes, escaped);
                if let Some(eb) = else_branch {
                    check_escapes_in_stmts(eb, candidates, classes, escaped);
                }
            }
            Stmt::While { condition, body } => {
                check_escapes_in_expr(condition, candidates, classes, escaped);
                check_escapes_in_stmts(body, candidates, classes, escaped);
            }
            Stmt::DoWhile { body, condition } => {
                check_escapes_in_stmts(body, candidates, classes, escaped);
                check_escapes_in_expr(condition, candidates, classes, escaped);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    check_escapes_in_stmts(
                        std::slice::from_ref(init_stmt),
                        candidates,
                        classes,
                        escaped,
                    );
                }
                if let Some(cond) = condition {
                    check_escapes_in_expr(cond, candidates, classes, escaped);
                }
                if let Some(upd) = update {
                    check_escapes_in_expr(upd, candidates, classes, escaped);
                }
                check_escapes_in_stmts(body, candidates, classes, escaped);
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                check_escapes_in_expr(discriminant, candidates, classes, escaped);
                for case in cases {
                    if let Some(test) = &case.test {
                        check_escapes_in_expr(test, candidates, classes, escaped);
                    }
                    check_escapes_in_stmts(&case.body, candidates, classes, escaped);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                check_escapes_in_stmts(body, candidates, classes, escaped);
                if let Some(c) = catch {
                    check_escapes_in_stmts(&c.body, candidates, classes, escaped);
                }
                if let Some(f) = finally {
                    check_escapes_in_stmts(f, candidates, classes, escaped);
                }
            }
            Stmt::Labeled { body, .. } => {
                check_escapes_in_stmts(
                    std::slice::from_ref(body.as_ref()),
                    candidates,
                    classes,
                    escaped,
                );
            }
            _ => {}
        }
    }
}

/// Check whether a candidate local escapes through the given expression.
///
/// A `LocalGet(id)` is SAFE only if it appears in:
///   - `PropertyGet { object: LocalGet(id), property }` — reading a field
///   - `PropertySet { object: LocalGet(id), property, value }` — writing a
///     field (but value must NOT contain LocalGet(id))
///   - `PropertyUpdate { object: LocalGet(id), .. }` — incrementing a field
///
/// `LocalSet(id, _)` anywhere marks it as escaped (reassignment).
///
/// Any other occurrence of `LocalGet(id)` marks it as escaped.
pub fn check_escapes_in_expr(
    e: &perry_hir::Expr,
    candidates: &std::collections::HashMap<u32, String>,
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
    escaped: &mut HashSet<u32>,
) {
    use perry_hir::{ArrayElement, CallArg, Expr};

    match e {
        // Safe uses: PropertyGet on a candidate local — *unless* the
        // property is a getter on the candidate's class. A getter is
        // dispatched as a real method call that takes `this` as a
        // function arg, so the receiver MUST be a real heap pointer,
        // not the scalar-replaced field set. Without this check,
        // `let r = new C(...); r.gettableProp` keeps `r` scalar-
        // replaced, the constructor never runs (its body is folded
        // into per-field stores), and the getter's `this_arg` reads
        // an uninitialized alloca → segfault. (Method calls are
        // already covered: they're wrapped in `Expr::Call` and the
        // Call/CallSpread arms below mark the receiver escaped.)
        Expr::PropertyGet { object, property } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if let Some(class_name) = candidates.get(id) {
                    if is_class_getter(classes, class_name, property) {
                        escaped.insert(*id);
                        return;
                    }
                    // Plain field read — safe, don't recurse into object.
                    return;
                }
            }
            // Not a candidate or not a LocalGet — recurse normally
            check_escapes_in_expr(object, candidates, classes, escaped);
        }

        // Safe uses: PropertySet on a candidate local — *unless* the
        // property is a setter (which dispatches as a real method call
        // and needs a heap-resident `this`). Otherwise treat as a plain
        // field write; value must not self-reference the candidate.
        Expr::PropertySet {
            object,
            value,
            property,
        } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if let Some(class_name) = candidates.get(id) {
                    if is_class_setter(classes, class_name, property) {
                        escaped.insert(*id);
                        check_escapes_in_expr(value, candidates, classes, escaped);
                        return;
                    }
                    // Object position is safe. But check if value contains
                    // LocalGet(id) — that would be self-referential escape.
                    if expr_contains_local_get(value, *id) {
                        escaped.insert(*id);
                    }
                    // Walk value for OTHER candidate escapes
                    check_escapes_in_expr(value, candidates, classes, escaped);
                    return;
                }
            }
            check_escapes_in_expr(object, candidates, classes, escaped);
            check_escapes_in_expr(value, candidates, classes, escaped);
        }

        // Safe uses: PropertyUpdate on a candidate local — *unless* the
        // property is a getter+setter pair (both fire on `obj.x++`).
        Expr::PropertyUpdate {
            object, property, ..
        } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if let Some(class_name) = candidates.get(id) {
                    if is_class_getter(classes, class_name, property)
                        || is_class_setter(classes, class_name, property)
                    {
                        escaped.insert(*id);
                        return;
                    }
                    // Safe — field increment on a non-escaping local
                    return;
                }
            }
            check_escapes_in_expr(object, candidates, classes, escaped);
        }

        // LocalSet: reassignment — always an escape
        Expr::LocalSet(id, value) => {
            if candidates.contains_key(id) {
                escaped.insert(*id);
            }
            check_escapes_in_expr(value, candidates, classes, escaped);
        }

        // LocalGet in any OTHER position (not already handled above) = escape
        Expr::LocalGet(id) => {
            if candidates.contains_key(id) {
                escaped.insert(*id);
            }
        }

        // New { args } — the New is the definition site for the candidate,
        // but args can escape OTHER candidates
        Expr::New { args, .. } => {
            for a in args {
                check_escapes_in_expr(a, candidates, classes, escaped);
            }
        }

        // Closure bodies: LocalGet(id) inside a closure is always an escape
        // because the closure can outlive the stack frame
        Expr::Closure { body, captures, .. } => {
            // Any captured candidate is an escape
            for c in captures {
                if candidates.contains_key(c) {
                    escaped.insert(*c);
                }
            }
            // Walk body too — closures can reference locals without explicitly
            // listing them in captures (the capture list may be incomplete at
            // this stage)
            check_escapes_in_stmts(body, candidates, classes, escaped);
        }

        // ── Recurse into all sub-expressions ──
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            check_escapes_in_expr(left, candidates, classes, escaped);
            check_escapes_in_expr(right, candidates, classes, escaped);
        }
        Expr::Unary { operand, .. }
        | Expr::Void(operand)
        | Expr::TypeOf(operand)
        | Expr::Await(operand)
        | Expr::Delete(operand)
        | Expr::StringCoerce(operand)
        | Expr::BooleanCoerce(operand)
        | Expr::NumberCoerce(operand)
        | Expr::IsFinite(operand)
        | Expr::IsNaN(operand)
        | Expr::NumberIsNaN(operand)
        | Expr::NumberIsFinite(operand)
        | Expr::NumberIsInteger(operand)
        | Expr::IsUndefinedOrBareNan(operand)
        | Expr::ParseFloat(operand)
        | Expr::ObjectKeys(operand)
        | Expr::ObjectValues(operand)
        | Expr::ObjectEntries(operand)
        | Expr::SetSize(operand)
        | Expr::MathSqrt(operand)
        | Expr::MathFloor(operand)
        | Expr::MathCeil(operand)
        | Expr::MathRound(operand)
        | Expr::MathAbs(operand)
        | Expr::MathMinSpread(operand)
        | Expr::MathMaxSpread(operand)
        | Expr::ArrayFrom(operand)
        | Expr::Uint8ArrayFrom(operand)
        | Expr::JsonParse(operand)
        | Expr::JsonStringify(operand)
        | Expr::IteratorToArray(operand)
        | Expr::WeakRefNew(operand)
        | Expr::WeakRefDeref(operand)
        | Expr::FinalizationRegistryNew(operand)
        | Expr::StructuredClone(operand)
        | Expr::QueueMicrotask(operand)
        | Expr::ProcessNextTick(operand)
        | Expr::ArrayIsArray(operand) => {
            check_escapes_in_expr(operand, candidates, classes, escaped);
        }
        Expr::JsonParseTyped { text, .. } => {
            check_escapes_in_expr(text, candidates, classes, escaped);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            check_escapes_in_expr(condition, candidates, classes, escaped);
            check_escapes_in_expr(then_expr, candidates, classes, escaped);
            check_escapes_in_expr(else_expr, candidates, classes, escaped);
        }
        Expr::Call { callee, args, .. } => {
            // Method-call form: `local.method(...)` needs a real heap `this`
            // pointer. HIR exact-receiver inlining is the layer that may prove
            // a safe `return this.field` replacement; if a method call reaches
            // codegen as a call, keep the receiver allocated.
            if let Expr::PropertyGet { object, .. } = callee.as_ref() {
                if let Expr::LocalGet(id) = object.as_ref() {
                    if candidates.contains_key(id) {
                        escaped.insert(*id);
                    }
                }
            }
            check_escapes_in_expr(callee, candidates, classes, escaped);
            for a in args {
                check_escapes_in_expr(a, candidates, classes, escaped);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            if let Expr::PropertyGet { object, .. } = callee.as_ref() {
                if let Expr::LocalGet(id) = object.as_ref() {
                    if candidates.contains_key(id) {
                        escaped.insert(*id);
                    }
                }
            }
            check_escapes_in_expr(callee, candidates, classes, escaped);
            for a in args {
                match a {
                    CallArg::Expr(e) | CallArg::Spread(e) => {
                        check_escapes_in_expr(e, candidates, classes, escaped);
                    }
                }
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                check_escapes_in_expr(o, candidates, classes, escaped);
            }
            for a in args {
                check_escapes_in_expr(a, candidates, classes, escaped);
            }
        }
        Expr::IndexGet { object, index } => {
            check_escapes_in_expr(object, candidates, classes, escaped);
            check_escapes_in_expr(index, candidates, classes, escaped);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            check_escapes_in_expr(object, candidates, classes, escaped);
            check_escapes_in_expr(index, candidates, classes, escaped);
            check_escapes_in_expr(value, candidates, classes, escaped);
        }
        Expr::Array(elements) => {
            for el in elements {
                check_escapes_in_expr(el, candidates, classes, escaped);
            }
        }
        Expr::ArraySpread(elements) => {
            for el in elements {
                match el {
                    ArrayElement::Expr(e) | ArrayElement::Spread(e) => {
                        check_escapes_in_expr(e, candidates, classes, escaped);
                    }
                }
            }
        }
        Expr::Object(props) => {
            for (_, v) in props {
                check_escapes_in_expr(v, candidates, classes, escaped);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, e) in parts {
                check_escapes_in_expr(e, candidates, classes, escaped);
            }
        }
        Expr::ArrayMap { array, callback }
        | Expr::ArrayFilter { array, callback }
        | Expr::ArraySome { array, callback }
        | Expr::ArrayEvery { array, callback }
        | Expr::ArrayFind { array, callback }
        | Expr::ArrayFindIndex { array, callback }
        | Expr::ArrayFindLast { array, callback }
        | Expr::ArrayFindLastIndex { array, callback }
        | Expr::ArrayForEach { array, callback }
        | Expr::ArrayFlatMap { array, callback }
        | Expr::ArraySort {
            array,
            comparator: callback,
        } => {
            check_escapes_in_expr(array, candidates, classes, escaped);
            check_escapes_in_expr(callback, candidates, classes, escaped);
        }
        Expr::ArrayReduce {
            array,
            callback,
            initial,
        }
        | Expr::ArrayReduceRight {
            array,
            callback,
            initial,
        } => {
            check_escapes_in_expr(array, candidates, classes, escaped);
            check_escapes_in_expr(callback, candidates, classes, escaped);
            if let Some(init) = initial {
                check_escapes_in_expr(init, candidates, classes, escaped);
            }
        }
        Expr::ArrayPush { array_id, value } => {
            if candidates.contains_key(array_id) {
                escaped.insert(*array_id);
            }
            check_escapes_in_expr(value, candidates, classes, escaped);
        }
        Expr::ArrayPop(id) | Expr::ArrayShift(id) => {
            if candidates.contains_key(id) {
                escaped.insert(*id);
            }
        }
        Expr::ArraySplice {
            array_id,
            start,
            delete_count,
            items,
        } => {
            if candidates.contains_key(array_id) {
                escaped.insert(*array_id);
            }
            check_escapes_in_expr(start, candidates, classes, escaped);
            if let Some(d) = delete_count {
                check_escapes_in_expr(d, candidates, classes, escaped);
            }
            for it in items {
                check_escapes_in_expr(it, candidates, classes, escaped);
            }
        }
        Expr::Sequence(es) => {
            for e in es {
                check_escapes_in_expr(e, candidates, classes, escaped);
            }
        }
        Expr::Update { id, .. } => {
            // Update on a candidate's id means it's being ++/-- directly
            // which would make no sense for an object — mark as escape
            if candidates.contains_key(id) {
                escaped.insert(*id);
            }
        }
        Expr::MapSet { map, key, value } => {
            check_escapes_in_expr(map, candidates, classes, escaped);
            check_escapes_in_expr(key, candidates, classes, escaped);
            check_escapes_in_expr(value, candidates, classes, escaped);
        }
        Expr::MapGet { map, key } | Expr::MapHas { map, key } | Expr::MapDelete { map, key } => {
            check_escapes_in_expr(map, candidates, classes, escaped);
            check_escapes_in_expr(key, candidates, classes, escaped);
        }
        Expr::SetAdd { set_id, value } => {
            if candidates.contains_key(set_id) {
                escaped.insert(*set_id);
            }
            check_escapes_in_expr(value, candidates, classes, escaped);
        }
        Expr::SetHas { set, value } | Expr::SetDelete { set, value } => {
            check_escapes_in_expr(set, candidates, classes, escaped);
            check_escapes_in_expr(value, candidates, classes, escaped);
        }
        Expr::MathPow(a, b)
        | Expr::PathJoin(a, b)
        | Expr::PathWin32Join(a, b)
        | Expr::ObjectIs(a, b)
        | Expr::ObjectHasOwn(a, b) => {
            check_escapes_in_expr(a, candidates, classes, escaped);
            check_escapes_in_expr(b, candidates, classes, escaped);
        }
        Expr::MathMin(values) | Expr::MathMax(values) => {
            for v in values {
                check_escapes_in_expr(v, candidates, classes, escaped);
            }
        }
        Expr::ErrorNew(opt) => {
            if let Some(o) = opt {
                check_escapes_in_expr(o, candidates, classes, escaped);
            }
        }
        Expr::ArrayJoin { array, separator } => {
            check_escapes_in_expr(array, candidates, classes, escaped);
            if let Some(sep) = separator {
                check_escapes_in_expr(sep, candidates, classes, escaped);
            }
        }
        Expr::ArraySlice { array, start, end } => {
            check_escapes_in_expr(array, candidates, classes, escaped);
            check_escapes_in_expr(start, candidates, classes, escaped);
            if let Some(e) = end {
                check_escapes_in_expr(e, candidates, classes, escaped);
            }
        }
        Expr::ArrayIncludes { array, value } | Expr::ArrayIndexOf { array, value } => {
            check_escapes_in_expr(array, candidates, classes, escaped);
            check_escapes_in_expr(value, candidates, classes, escaped);
        }
        Expr::NewDynamic { callee, args } => {
            check_escapes_in_expr(callee, candidates, classes, escaped);
            for a in args {
                check_escapes_in_expr(a, candidates, classes, escaped);
            }
        }
        Expr::FetchWithOptions {
            url,
            method,
            body,
            headers,
        } => {
            check_escapes_in_expr(url, candidates, classes, escaped);
            check_escapes_in_expr(method, candidates, classes, escaped);
            check_escapes_in_expr(body, candidates, classes, escaped);
            for (_, v) in headers {
                check_escapes_in_expr(v, candidates, classes, escaped);
            }
        }
        Expr::SuperCall(args)
        | Expr::StaticMethodCall { args, .. }
        | Expr::SuperMethodCall { args, .. } => {
            for a in args {
                check_escapes_in_expr(a, candidates, classes, escaped);
            }
        }
        Expr::I18nString { params, .. } => {
            for (_, v) in params {
                check_escapes_in_expr(v, candidates, classes, escaped);
            }
        }
        Expr::Yield { value, .. } => {
            if let Some(v) = value {
                check_escapes_in_expr(v, candidates, classes, escaped);
            }
        }
        Expr::ParseInt { string, radix } => {
            check_escapes_in_expr(string, candidates, classes, escaped);
            if let Some(r) = radix {
                check_escapes_in_expr(r, candidates, classes, escaped);
            }
        }
        Expr::JsonStringifyFull(value, replacer, indent) => {
            check_escapes_in_expr(value, candidates, classes, escaped);
            check_escapes_in_expr(replacer, candidates, classes, escaped);
            check_escapes_in_expr(indent, candidates, classes, escaped);
        }
        Expr::RegExpTest { regex, string } | Expr::RegExpExec { regex, string } => {
            check_escapes_in_expr(regex, candidates, classes, escaped);
            check_escapes_in_expr(string, candidates, classes, escaped);
        }
        Expr::In { property, object } => {
            check_escapes_in_expr(property, candidates, classes, escaped);
            check_escapes_in_expr(object, candidates, classes, escaped);
        }
        Expr::InstanceOf { expr, .. } => {
            check_escapes_in_expr(expr, candidates, classes, escaped);
        }
        Expr::ObjectRest { object, .. } => {
            check_escapes_in_expr(object, candidates, classes, escaped);
        }
        Expr::StaticFieldSet { value, .. } => {
            check_escapes_in_expr(value, candidates, classes, escaped);
        }
        Expr::ProcessOn { event, handler } => {
            check_escapes_in_expr(event, candidates, classes, escaped);
            check_escapes_in_expr(handler, candidates, classes, escaped);
        }
        Expr::FsWriteFileSync(a, b)
        | Expr::JsonParseReviver {
            text: a,
            reviver: b,
        }
        | Expr::JsonParseWithReviver(a, b)
        | Expr::PathRelative(a, b)
        | Expr::PathMatchesGlob(a, b)
        | Expr::PathResolveJoin(a, b) => {
            check_escapes_in_expr(a, candidates, classes, escaped);
            check_escapes_in_expr(b, candidates, classes, escaped);
        }
        Expr::FinalizationRegistryRegister {
            registry,
            target,
            held,
            token,
        } => {
            check_escapes_in_expr(registry, candidates, classes, escaped);
            check_escapes_in_expr(target, candidates, classes, escaped);
            check_escapes_in_expr(held, candidates, classes, escaped);
            if let Some(t) = token {
                check_escapes_in_expr(t, candidates, classes, escaped);
            }
        }
        Expr::FinalizationRegistryUnregister { registry, token } => {
            check_escapes_in_expr(registry, candidates, classes, escaped);
            check_escapes_in_expr(token, candidates, classes, escaped);
        }
        Expr::ArrayFromMapped { iterable, map_fn }
        | Expr::ObjectGroupBy {
            items: iterable,
            key_fn: map_fn,
        } => {
            check_escapes_in_expr(iterable, candidates, classes, escaped);
            check_escapes_in_expr(map_fn, candidates, classes, escaped);
        }
        Expr::ArrayToSorted { array, comparator } => {
            check_escapes_in_expr(array, candidates, classes, escaped);
            if let Some(c) = comparator {
                check_escapes_in_expr(c, candidates, classes, escaped);
            }
        }
        Expr::ArrayToReversed { array }
        | Expr::ArrayFlat { array }
        | Expr::ArrayEntries(array)
        | Expr::ArrayKeys(array)
        | Expr::ArrayValues(array) => {
            check_escapes_in_expr(array, candidates, classes, escaped);
        }
        Expr::ArrayToSpliced {
            array,
            start,
            delete_count,
            items,
        } => {
            check_escapes_in_expr(array, candidates, classes, escaped);
            check_escapes_in_expr(start, candidates, classes, escaped);
            check_escapes_in_expr(delete_count, candidates, classes, escaped);
            for it in items {
                check_escapes_in_expr(it, candidates, classes, escaped);
            }
        }
        Expr::ArrayWith {
            array,
            index,
            value,
        } => {
            check_escapes_in_expr(array, candidates, classes, escaped);
            check_escapes_in_expr(index, candidates, classes, escaped);
            check_escapes_in_expr(value, candidates, classes, escaped);
        }
        Expr::ArrayCopyWithin {
            target, start, end, ..
        } => {
            check_escapes_in_expr(target, candidates, classes, escaped);
            check_escapes_in_expr(start, candidates, classes, escaped);
            if let Some(e) = end {
                check_escapes_in_expr(e, candidates, classes, escaped);
            }
        }
        Expr::ArrayAt { array, index } => {
            check_escapes_in_expr(array, candidates, classes, escaped);
            check_escapes_in_expr(index, candidates, classes, escaped);
        }
        Expr::ArrayUnshift { value, .. } => {
            check_escapes_in_expr(value, candidates, classes, escaped);
        }
        Expr::TypedArrayNew { arg, .. } => {
            if let Some(a) = arg {
                check_escapes_in_expr(a, candidates, classes, escaped);
            }
        }
        Expr::ChildProcessExecSync { command, options } => {
            check_escapes_in_expr(command, candidates, classes, escaped);
            if let Some(o) = options {
                check_escapes_in_expr(o, candidates, classes, escaped);
            }
        }
        Expr::ChildProcessSpawnSync {
            command,
            args,
            options,
        }
        | Expr::ChildProcessSpawn {
            command,
            args,
            options,
        } => {
            check_escapes_in_expr(command, candidates, classes, escaped);
            if let Some(a) = args {
                check_escapes_in_expr(a, candidates, classes, escaped);
            }
            if let Some(o) = options {
                check_escapes_in_expr(o, candidates, classes, escaped);
            }
        }
        Expr::ChildProcessExec {
            command,
            options,
            callback,
        } => {
            check_escapes_in_expr(command, candidates, classes, escaped);
            if let Some(o) = options {
                check_escapes_in_expr(o, candidates, classes, escaped);
            }
            if let Some(c) = callback {
                check_escapes_in_expr(c, candidates, classes, escaped);
            }
        }
        Expr::ChildProcessSpawnBackground {
            command,
            args,
            log_file,
            env_json,
        } => {
            check_escapes_in_expr(command, candidates, classes, escaped);
            if let Some(a) = args {
                check_escapes_in_expr(a, candidates, classes, escaped);
            }
            check_escapes_in_expr(log_file, candidates, classes, escaped);
            if let Some(e) = env_json {
                check_escapes_in_expr(e, candidates, classes, escaped);
            }
        }
        Expr::ChildProcessGetProcessStatus(h) | Expr::ChildProcessKillProcess(h) => {
            check_escapes_in_expr(h, candidates, classes, escaped);
        }
        Expr::FetchGetWithAuth { url, auth_header } => {
            check_escapes_in_expr(url, candidates, classes, escaped);
            check_escapes_in_expr(auth_header, candidates, classes, escaped);
        }
        Expr::FetchPostWithAuth {
            url,
            auth_header,
            body,
        } => {
            check_escapes_in_expr(url, candidates, classes, escaped);
            check_escapes_in_expr(auth_header, candidates, classes, escaped);
            check_escapes_in_expr(body, candidates, classes, escaped);
        }
        Expr::SetNewFromArray(arr) => check_escapes_in_expr(arr, candidates, classes, escaped),
        Expr::Atob(o) | Expr::Btoa(o) => check_escapes_in_expr(o, candidates, classes, escaped),
        Expr::JsonStringifyPretty {
            value,
            replacer,
            space,
        } => {
            check_escapes_in_expr(value, candidates, classes, escaped);
            if let Some(r) = replacer {
                check_escapes_in_expr(r, candidates, classes, escaped);
            }
            check_escapes_in_expr(space, candidates, classes, escaped);
        }
        Expr::PathBasenameExt(a, b) => {
            check_escapes_in_expr(a, candidates, classes, escaped);
            check_escapes_in_expr(b, candidates, classes, escaped);
        }
        // Leaf expressions that don't contain LocalGet — no escape possible
        Expr::Integer(_)
        | Expr::Number(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::Undefined
        | Expr::Null
        | Expr::This
        | Expr::FuncRef(_)
        | Expr::ClassRef(_)
        | Expr::ExternFuncRef { .. }
        | Expr::GlobalGet(_)
        | Expr::DateNow
        | Expr::PerformanceNow
        | Expr::MapNew
        | Expr::SetNew
        | Expr::EnumMember { .. }
        | Expr::StaticFieldGet { .. }
        | Expr::RegExp { .. }
        | Expr::Uint8ArrayNew(None)
        // #853: `Expr::ErrorNew(opt)` is already matched by the earlier
        // arm (around line 4949). The `ErrorNew(None)` here was dead —
        // removed.
        | Expr::BigInt(_) => {}
        // Catch-all: conservatively mark any candidate referenced in an
        // unrecognized expression as escaped. This is safe — just misses
        // the optimization for patterns we haven't enumerated.
        _ => {
            mark_all_candidate_refs_in_expr(e, candidates, escaped);
        }
    }
}

/// Helper: does this expression contain `LocalGet(target_id)` anywhere?
pub fn expr_contains_local_get(e: &perry_hir::Expr, target_id: u32) -> bool {
    use perry_hir::Expr;
    match e {
        Expr::LocalGet(id) => *id == target_id,
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            expr_contains_local_get(left, target_id) || expr_contains_local_get(right, target_id)
        }
        Expr::Unary { operand, .. }
        | Expr::Void(operand)
        | Expr::TypeOf(operand)
        | Expr::Await(operand)
        | Expr::StringCoerce(operand)
        | Expr::NumberCoerce(operand)
        | Expr::BooleanCoerce(operand)
        | Expr::Delete(operand) => expr_contains_local_get(operand, target_id),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            expr_contains_local_get(condition, target_id)
                || expr_contains_local_get(then_expr, target_id)
                || expr_contains_local_get(else_expr, target_id)
        }
        Expr::Call { callee, args, .. } => {
            expr_contains_local_get(callee, target_id)
                || args.iter().any(|a| expr_contains_local_get(a, target_id))
        }
        Expr::PropertyGet { object, .. } | Expr::PropertyUpdate { object, .. } => {
            expr_contains_local_get(object, target_id)
        }
        Expr::PropertySet { object, value, .. } => {
            expr_contains_local_get(object, target_id) || expr_contains_local_get(value, target_id)
        }
        Expr::IndexGet { object, index } => {
            expr_contains_local_get(object, target_id) || expr_contains_local_get(index, target_id)
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            expr_contains_local_get(object, target_id)
                || expr_contains_local_get(index, target_id)
                || expr_contains_local_get(value, target_id)
        }
        Expr::LocalSet(_, value) => expr_contains_local_get(value, target_id),
        Expr::Array(elements) => elements
            .iter()
            .any(|e| expr_contains_local_get(e, target_id)),
        Expr::Object(props) => props
            .iter()
            .any(|(_, v)| expr_contains_local_get(v, target_id)),
        Expr::New { args, .. } => args.iter().any(|a| expr_contains_local_get(a, target_id)),
        Expr::Sequence(es) => es.iter().any(|e| expr_contains_local_get(e, target_id)),
        Expr::Update { id, .. } => *id == target_id,
        _ => false, // Conservative: we don't recurse into everything, but false means "not found" which is safe
    }
}

/// Conservative catch-all: walk the expression and mark any candidate
/// local referenced via LocalGet as escaped. Used for Expr variants we
/// haven't explicitly enumerated in check_escapes_in_expr.
///
/// **Safety note (issue #150):** `collect_ref_ids_in_expr` has a silent
/// `_ => {}` fallthrough for unenumerated HIR variants. That means for
/// variants like `ObjectGetOwnPropertyDescriptor(LocalGet(p), key)` — which
/// is an identity-observing operation that should escape `p` — the collector
/// returns an empty set, and `p` ends up scalar-replaced while an external
/// runtime function (`js_object_get_own_property_descriptor`) tries to
/// dereference its dummy alloca slot. Since we can't enumerate every HIR
/// variant that might embed a LocalGet, we conservatively mark EVERY
/// candidate as escaped whenever this catch-all fires. The cost is losing
/// scalar replacement in functions that happen to contain an un-enumerated
/// variant anywhere; the safety is not silently miscompiling identity-
/// observing code. This mirrors the `check_object_literal_escapes_in_expr`
/// catch-all at line ~4148 which already does exactly this for object
/// literal candidates.
pub fn mark_all_candidate_refs_in_expr(
    e: &perry_hir::Expr,
    candidates: &std::collections::HashMap<u32, String>,
    escaped: &mut HashSet<u32>,
) {
    // First pass: walk what collect_ref_ids_in_expr knows about — these are
    // the references we can prove exist.
    let mut refs: HashSet<u32> = HashSet::new();
    collect_ref_ids_in_expr(e, &mut refs);
    for id in refs {
        if candidates.contains_key(&id) {
            escaped.insert(id);
        }
    }
    // Second pass: conservative fallback. We're in the check_escapes_in_expr
    // catch-all, meaning `e` is some HIR variant not explicitly enumerated
    // there. The collector above may have silently skipped unknown
    // sub-variants, so we must assume any candidate in scope could be
    // referenced transitively. Mark them all escaped.
    for id in candidates.keys() {
        escaped.insert(*id);
    }
}

// ── Escape analysis for scalar replacement of non-escaping array literals ──

/// Upper bound on array length for scalar replacement. Larger literals pay
/// per-element alloca + store even when every slot is dead, and the gain over
/// the exact-sized arena allocator shrinks as N grows. 16 matches the old
/// `MIN_ARRAY_CAPACITY` ceiling so we cover every size the previous allocator
/// would have padded anyway.
pub(crate) const MAX_SCALAR_ARRAY_LEN: usize = 16;
