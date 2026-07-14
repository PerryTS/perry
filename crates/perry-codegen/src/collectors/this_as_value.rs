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
    let mut seen_parent_names: HashSet<&str> = HashSet::new();
    let mut parent_depth = 0usize;
    while let Some(p) = parent {
        if !seen_parent_names.insert(p) || parent_depth > 64 {
            break;
        }
        parent_depth += 1;
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
    let mut seen_parent_names: HashSet<&str> = HashSet::new();
    let mut parent_depth = 0usize;
    while let Some(p) = parent {
        if !seen_parent_names.insert(p) || parent_depth > 64 {
            break;
        }
        parent_depth += 1;
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
    let mut seen_parent_names: HashSet<String> = HashSet::new();
    let mut depth = 0usize;
    while let Some(name) = cur {
        if !seen_parent_names.insert(name.clone()) {
            break;
        }
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

/// Issue #6343: walk the class's `extends` chain and report whether it reaches
/// a base whose *construction* codegen cannot see.
///
/// Scalar replacement models an instance as exactly the set of DECLARED fields
/// on its chain — one alloca per field name — and inlines the constructor
/// bodies that fill them. That is only faithful when the whole chain is
/// visible. A base that isn't contributes instance state the promoted set does
/// not model, and every way that happens is silent:
///
///   * a **native** base — `class X extends EventEmitter`, `extends Readable`,
///     … — installs its method surface as OWN PROPERTIES on the instance at
///     subclass-init time (`js_object_set_field_by_name(obj, "emit", <native
///     closure>)`) rather than on a prototype. `emit` has no declared slot, so
///     the promoted set has no slot for it and `x.emit` reads back
///     `undefined` (#6343: `class X extends EventEmitter { a = 1 }` printed
///     `typeof x.emit === "undefined"` while `x.a` was correct).
///   * a **dynamic** base — `extends <expr>`, including a lexically shadowed
///     heritage name — runs an arbitrary constructor that can install
///     anything.
///   * a parent NAME that resolves to no visible class: a builtin (`Error`,
///     `Map`, `Set`, `Event`, …) or a base whose declaration never reached
///     this module. Its construction happens in the runtime, not in code
///     codegen can inline.
///
/// The only sound answer for all three is to keep the instance on the heap,
/// where the real init runs and property lookup goes through the object.
///
/// This is deliberately a chain property, not a name test: it must hop user
/// classes (`class Leaf extends Mid`, `class Mid extends EventEmitter`) and it
/// must NOT fire for a chain that bottoms out in an ordinary user class — that
/// instance is fully modeled and keeping it scalar-replaced is a real win
/// (guarded by `scripts/run_issue_945_scalar_method_ir_guard.sh`).
///
/// Generalizes [`class_chain_extends_builtin_error`] (#573), which stays as-is
/// because it is name-keyed and therefore also fires for a *locally shadowed*
/// `Error` — this walk resolves such a shadow to the user class and would let
/// it through.
pub fn class_chain_has_unmodeled_base(
    class: &perry_hir::Class,
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
) -> bool {
    let mut current = class;
    let mut seen: HashSet<String> = HashSet::new();
    loop {
        // A cycle (or a chain deep enough to look like one) means the walk
        // can't prove anything. Fail closed: keep the instance on the heap.
        if !seen.insert(current.name.clone()) || seen.len() > 64 {
            return true;
        }
        // `native_extends` is the (module, class) tag for a base whose
        // subclass-init shim stamps a surface onto `this` at construction —
        // events, node:stream, the Web Streams bases, async_hooks, ws.
        if current.native_extends.is_some() {
            return true;
        }
        // `class X extends <expr>` — the parent is a runtime value; its
        // constructor is opaque to this analysis.
        if current.extends_expr.is_some() || current.heritage_lexically_shadowed {
            return true;
        }
        let Some(parent_name) = current.extends_name.as_deref() else {
            // Chain bottoms out in a root user class: fully modeled.
            return false;
        };
        match classes.get(parent_name) {
            Some(parent) => current = parent,
            // A parent name with no visible class behind it — a builtin, or an
            // import whose stub never landed. Unmodeled either way.
            None => return true,
        }
    }
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
            Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => false,
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
        | Expr::SuperMethodCallSpread { .. }
        | Expr::SuperPropertySet { .. }
        | Expr::ObjectSuperPropertyGet { .. }
        | Expr::ObjectSuperPropertySet { .. }
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
        // #4126 lowers `this.field = x` ctor stores as `PutValueSet` (with
        // `target` and `receiver` both `Expr::This`) instead of `PropertySet`.
        // Mirror the `PropertySet { object: This }` rule: a plain field store
        // is the safe, scalar-replaceable pattern — only a `this.<method>`
        // write or a `this` materialized in the value counts as this-as-value.
        // Without this, every field-assigning constructor marks its class as
        // using `this` as a value, forcing all instances onto the heap path
        // (regressed scalar replacement / #945).
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            ..
        } => {
            let target_unsafe = if matches!(target.as_ref(), Expr::This) {
                match key.as_ref() {
                    Expr::String(property) => !fields.contains(property),
                    // Computed/dynamic key — can't prove it's a plain field.
                    _ => true,
                }
            } else {
                expr_uses_this_as_value(target, fields)
            };
            // For an ordinary `this.f = v` store the receiver mirrors the
            // (safe) `This` target; don't double-count it as this-as-value.
            let receiver_unsafe = if matches!(target.as_ref(), Expr::This)
                && matches!(receiver.as_ref(), Expr::This)
            {
                false
            } else {
                expr_uses_this_as_value(receiver, fields)
            };
            target_unsafe || receiver_unsafe || expr_uses_this_as_value(value, fields)
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
        Expr::NewDynamic { callee, args, .. } => {
            expr_uses_this_as_value(callee, fields)
                || args.iter().any(|a| expr_uses_this_as_value(a, fields))
        }
        Expr::LocalSet(_, value) => expr_uses_this_as_value(value, fields),
        Expr::Sequence(es) => es.iter().any(|e| expr_uses_this_as_value(e, fields)),
        Expr::Yield { value, .. } => value
            .as_ref()
            .is_some_and(|v| expr_uses_this_as_value(v, fields)),
        Expr::InstanceOf { expr, ty_expr, .. } => {
            expr_uses_this_as_value(expr, fields)
                || ty_expr
                    .as_ref()
                    .map(|t| expr_uses_this_as_value(t, fields))
                    .unwrap_or(false)
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::{Class, ClassField, Expr, Function, Stmt};
    use perry_types::Type;
    use std::collections::HashMap;

    fn function(name: &str, body: Vec<Stmt>) -> Function {
        Function {
            id: 0,
            name: name.to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Any,
            body,
            is_async: false,
            is_generator: false,
            is_strict: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }
    }

    fn field(name: &str) -> ClassField {
        ClassField {
            name: name.to_string(),
            key_expr: None,
            ty: Type::Any,
            init: None,
            is_private: false,
            is_readonly: false,
            decorators: Vec::new(),
        }
    }

    fn class(name: &str, extends_name: Option<&str>) -> Class {
        Class {
            id: 0,
            name: name.to_string(),
            type_params: Vec::new(),
            extends: None,
            extends_name: extends_name.map(str::to_string),
            native_extends: None,
            extends_expr: None,
            heritage_lexically_shadowed: false,
            fields: Vec::new(),
            constructor: None,
            methods: Vec::new(),
            getters: Vec::new(),
            setters: Vec::new(),
            static_fields: Vec::new(),
            static_methods: Vec::new(),
            computed_members: Vec::new(),
            decorators: Vec::new(),
            is_exported: false,
            aliases: Vec::new(),
            is_nested: false,
            static_accessor_names: Vec::new(),
            static_accessor_fn_ids: Vec::new(),
        }
    }

    #[test]
    fn this_as_value_parent_walk_stops_on_cyclic_parent_chain() {
        let mut child = class("A", Some("B"));
        child.constructor = Some(function(
            "constructor",
            vec![Stmt::Return(Some(Expr::PropertyGet {
                object: Box::new(Expr::This),
                property: "value".to_string(),
            }))],
        ));

        let mut parent = class("B", Some("A"));
        parent.fields.push(field("value"));

        let mut classes = HashMap::new();
        classes.insert(child.name.clone(), &child);
        classes.insert(parent.name.clone(), &parent);

        assert!(!class_uses_this_as_value(&child, &classes));
    }

    #[test]
    fn builtin_error_parent_walk_stops_on_cyclic_parent_chain() {
        let child = class("A", Some("B"));
        let parent = class("B", Some("A"));

        let mut classes = HashMap::new();
        classes.insert(child.name.clone(), &child);
        classes.insert(parent.name.clone(), &parent);

        assert!(!class_chain_extends_builtin_error(&child, &classes));
    }

    // ── #6343: unmodeled-base chain walk ──

    /// A root user class with no heritage is fully modeled — scalar
    /// replacement must stay available (this is the #945 fast path).
    #[test]
    fn unmodeled_base_allows_plain_class() {
        let plain = class("Plain", None);
        let mut classes = HashMap::new();
        classes.insert(plain.name.clone(), &plain);

        assert!(!class_chain_has_unmodeled_base(&plain, &classes));
    }

    /// A chain of ordinary user classes is fully modeled too.
    #[test]
    fn unmodeled_base_allows_user_class_chain() {
        let base = class("Base", None);
        let mid = class("Mid", Some("Base"));
        let leaf = class("Leaf", Some("Mid"));
        let mut classes = HashMap::new();
        classes.insert(base.name.clone(), &base);
        classes.insert(mid.name.clone(), &mid);
        classes.insert(leaf.name.clone(), &leaf);

        assert!(!class_chain_has_unmodeled_base(&leaf, &classes));
    }

    /// `class X extends EventEmitter` — the native base installs its surface as
    /// own properties on the instance, so the instance must stay on the heap.
    #[test]
    fn unmodeled_base_rejects_direct_native_parent() {
        let mut child = class("X", Some("EventEmitter"));
        child.native_extends = Some(("events".to_string(), "EventEmitter".to_string()));
        let mut classes = HashMap::new();
        classes.insert(child.name.clone(), &child);

        assert!(class_chain_has_unmodeled_base(&child, &classes));
    }

    /// The native base is found by walking the CHAIN, not by matching the
    /// leaf's own `extends` name: `class Leaf extends Mid`, `class Mid extends
    /// EventEmitter`.
    #[test]
    fn unmodeled_base_rejects_indirect_native_parent() {
        let mut mid = class("Mid", Some("EventEmitter"));
        mid.native_extends = Some(("events".to_string(), "EventEmitter".to_string()));
        let leaf = class("Leaf", Some("Mid"));
        let mut classes = HashMap::new();
        classes.insert(mid.name.clone(), &mid);
        classes.insert(leaf.name.clone(), &leaf);

        assert!(class_chain_has_unmodeled_base(&leaf, &classes));
    }

    /// A parent name with no class behind it (a builtin such as `Error` /
    /// `Map`, or an import whose stub never landed) is unmodeled.
    #[test]
    fn unmodeled_base_rejects_unresolvable_parent_name() {
        let child = class("MyError", Some("Error"));
        let mut classes = HashMap::new();
        classes.insert(child.name.clone(), &child);

        assert!(class_chain_has_unmodeled_base(&child, &classes));
    }

    /// `class X extends <expr>` — an arbitrary runtime parent value.
    #[test]
    fn unmodeled_base_rejects_dynamic_parent_expr() {
        let mut child = class("X", None);
        child.extends_expr = Some(Box::new(Expr::LocalGet(0)));
        let mut classes = HashMap::new();
        classes.insert(child.name.clone(), &child);

        assert!(class_chain_has_unmodeled_base(&child, &classes));
    }

    /// A cyclic chain proves nothing, so it must fail closed (escape).
    #[test]
    fn unmodeled_base_fails_closed_on_cyclic_parent_chain() {
        let child = class("A", Some("B"));
        let parent = class("B", Some("A"));
        let mut classes = HashMap::new();
        classes.insert(child.name.clone(), &child);
        classes.insert(parent.name.clone(), &parent);

        assert!(class_chain_has_unmodeled_base(&child, &classes));
    }
}
