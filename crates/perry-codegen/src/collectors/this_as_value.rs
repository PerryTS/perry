//! `this`-as-value detection for scalar replacement of `new` locals.
//!
//! Split out of `escape_news.rs` in v0.5.1021 to satisfy the file-size CI
//! gate. No behavior change — these functions remain `pub` and are re-
//! exported from `collectors/mod.rs`.

use std::collections::HashSet;

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
        Expr::SuperCall(_)
        | Expr::SuperMethodCall { .. }
        | Expr::ObjectSuperPropertyGet { .. }
        | Expr::ObjectSuperMethodCall { .. } => true,
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
        | Expr::ObjectCoerce(operand)
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
            ArrayElement::Hole => false,
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
