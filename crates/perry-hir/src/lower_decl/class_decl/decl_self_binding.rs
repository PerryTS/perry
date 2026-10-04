//! #11157: the lexical self-binding of a per-evaluation class DECLARATION.
//!
//! A class declared in a function body (including the body of a CommonJS
//! module wrapper) whose heritage is a runtime value or that has private
//! elements lowers to a `ClassExprFresh` object per evaluation. Its static
//! fields live on that object. Before this module, the class's own members
//! resolved its name to `ClassRef(<template>)`, so `C.count = C.count + 1` in
//! a static method read and wrote the shared template, never the object
//! whose statics were initialized, and a static-field arrow's `this` was
//! the template too. bson's `ObjectId.getInc()` (`class ObjectId extends
//! BSONValue` inside `bson.cjs`) returned 1 on every call as a result.
//!
//! Named class EXPRESSIONS already get a compiler-private self-binding local
//! (`class_expr_self_bindings`, see `arm_class.rs`) that codegen fills with
//! the evaluated class object before any static initializer runs. These
//! helpers give a class declaration the same binding when it is known up
//! front to take the fresh path.

use swc_ecma_ast as ast;

use crate::ir::{Expr, Stmt};
use crate::lower::LoweringContext;
use crate::types::LocalId;

/// Syntactic private-element check: the class will report
/// `has_private_elements()` once lowered.
/// Syntactic computed-key check: a ClassBody member whose key is computed.
pub(super) fn class_body_has_computed_keys(class: &ast::Class) -> bool {
    class.body.iter().any(|member| match member {
        ast::ClassMember::Method(m) => matches!(m.key, ast::PropName::Computed(_)),
        ast::ClassMember::ClassProp(p) => matches!(p.key, ast::PropName::Computed(_)),
        ast::ClassMember::AutoAccessor(a) => {
            matches!(a.key, ast::Key::Public(ast::PropName::Computed(_)))
        }
        _ => false,
    })
}

pub(super) fn class_body_has_private_names(class: &ast::Class) -> bool {
    class.body.iter().any(|member| match member {
        ast::ClassMember::PrivateMethod(_) | ast::ClassMember::PrivateProp(_) => true,
        ast::ClassMember::AutoAccessor(accessor) => {
            matches!(accessor.key, ast::Key::Private(_))
        }
        _ => false,
    })
}

/// #11759 (c′): whether a function-body or block class declaration may be
/// evaluated more than once, so its later evaluations must be fresh class
/// objects (its first one stays the shared class). `lower::run_once` decides;
/// a native-module parent stays on the template (#10623), and a runtime
/// heritage value already makes every evaluation fresh.
pub(crate) fn may_evaluate_repeatedly(
    ctx: &LoweringContext,
    span: swc_common::Span,
    dynamic_heritage: bool,
    native_parent: bool,
) -> bool {
    !dynamic_heritage && !native_parent && !ctx.class_definition_runs_once(span)
}

/// The names of `class`'s non-computed, non-private static fields.
pub(crate) fn declared_static_field_names(class: &ast::Class) -> Vec<String> {
    class
        .body
        .iter()
        .filter_map(|member| match member {
            ast::ClassMember::ClassProp(prop) if prop.is_static => match &prop.key {
                ast::PropName::Ident(ident) => Some(ident.sym.to_string()),
                ast::PropName::Str(s) => s.value.as_str().map(str::to_string),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// #11759 (c′): `new C(args)` through a local holding an evaluation of a
/// shared-first class declaration: the static construction when the local
/// holds the first evaluation (the shared class), else the by-value one.
/// Arguments holding a function or class are left by value: the guard
/// duplicates the arguments into both branches.
pub(crate) fn guard_shared_first_new(ctx: &LoweringContext, expr: Expr) -> Expr {
    let Expr::NewDynamic {
        callee,
        args,
        byte_offset,
    } = &expr
    else {
        return expr;
    };
    let Expr::LocalGet(binding) = callee.as_ref() else {
        return expr;
    };
    let Some((template, _)) = ctx.shared_first_class_bindings.get(binding) else {
        return expr;
    };
    if args.iter().any(defines_code) {
        return expr;
    }
    let template = template.clone();
    let mut static_args = args.clone();
    let captures: Vec<LocalId> = ctx
        .lookup_class_captures(&template)
        .map(<[_]>::to_vec)
        .unwrap_or_default();
    let cap_args_appended = captures.len() as u32;
    static_args.extend(captures.into_iter().map(Expr::LocalGet));
    let construct = Expr::New {
        class_name: template.clone(),
        args: static_args,
        type_args: Vec::new(),
        byte_offset: *byte_offset,
        cap_args_appended,
    };
    // A guarded class environment records the evaluation on an instance
    // built statically once the class has had several (`ClassEnvStamp`).
    let construct = if ctx.is_class_env_guarded(&template) {
        Expr::ClassEnvStamp {
            class_name: template.clone(),
            instance: Box::new(construct),
            evaluation: Box::new(Expr::LocalGet(*binding)),
        }
    } else {
        construct
    };
    Expr::Conditional {
        condition: Box::new(Expr::ClassIsFirstEvaluation {
            value: Box::new(Expr::LocalGet(*binding)),
            template,
        }),
        then_expr: Box::new(construct),
        else_expr: Box::new(expr),
    }
}

/// #11759 (c′): `C.<static field>` through a local holding an evaluation of a
/// shared-first class declaration: the template's static field when the local
/// holds the first evaluation, else the property of the evaluated object.
pub(crate) fn guard_shared_first_static_get(ctx: &LoweringContext, expr: Expr) -> Expr {
    let Expr::PropertyGet {
        object, property, ..
    } = &expr
    else {
        return expr;
    };
    let Expr::LocalGet(binding) = object.as_ref() else {
        return expr;
    };
    let Some((template, statics)) = ctx.shared_first_class_bindings.get(binding) else {
        return expr;
    };
    if !statics.iter().any(|name| name == property) {
        return expr;
    }
    Expr::Conditional {
        condition: Box::new(Expr::ClassIsFirstEvaluation {
            value: Box::new(Expr::LocalGet(*binding)),
            template: template.clone(),
        }),
        then_expr: Box::new(Expr::StaticFieldGet {
            class_name: template.clone(),
            field_name: property.clone(),
        }),
        else_expr: Box::new(expr),
    }
}

/// #11759 (c′): `C.<static method>(args)` through a local holding an
/// evaluation of a shared-first class declaration: the template's static
/// method call when the local holds the first evaluation (what a single
/// evaluation's `C.m()` is), else the call on the evaluated object. Arguments
/// holding a function or class are left by value, as for `new`.
pub(crate) fn guard_shared_first_static_call(ctx: &LoweringContext, expr: Expr) -> Expr {
    let Expr::Call { callee, args, .. } = &expr else {
        return expr;
    };
    let Expr::PropertyGet {
        object, property, ..
    } = callee.as_ref()
    else {
        return expr;
    };
    let Expr::LocalGet(binding) = object.as_ref() else {
        return expr;
    };
    let Some((template, _)) = ctx.shared_first_class_bindings.get(binding) else {
        return expr;
    };
    if !ctx.has_static_method(template, property) || args.iter().any(defines_code) {
        return expr;
    }
    Expr::Conditional {
        condition: Box::new(Expr::ClassIsFirstEvaluation {
            value: Box::new(Expr::LocalGet(*binding)),
            template: template.clone(),
        }),
        then_expr: Box::new(Expr::StaticMethodCall {
            class_name: template.clone(),
            method_name: property.clone(),
            args: args.clone(),
        }),
        else_expr: Box::new(expr),
    }
}

/// #11759 (c′): a template-keyed capture refresh (`RegisterClassCaptures`)
/// of a shared-first declaration runs only while the declaration's binding
/// holds its first evaluation; a later evaluation's captures live on its own
/// class object.
pub(crate) fn guard_shared_first_capture_snapshot(
    ctx: &LoweringContext,
    class_name: &str,
    snapshot: Stmt,
) -> Stmt {
    let Some(binding) = ctx.shared_first_decl_locals.get(class_name) else {
        return snapshot;
    };
    let Stmt::Expr(register) = snapshot else {
        return snapshot;
    };
    Stmt::Expr(Expr::Conditional {
        condition: Box::new(Expr::ClassIsFirstEvaluation {
            value: Box::new(Expr::LocalGet(*binding)),
            template: class_name.to_string(),
        }),
        then_expr: Box::new(register),
        else_expr: Box::new(Expr::Undefined),
    })
}

/// Does `expr` define a function or class (whose duplicate would be a second
/// definition)?
fn defines_code(expr: &Expr) -> bool {
    let mut found = false;
    let mut visit = |e: &Expr| {
        if matches!(e, Expr::Closure { .. } | Expr::ClassExprFresh { .. }) {
            found = true;
        }
    };
    walk_defines(expr, &mut visit);
    found
}

fn walk_defines(expr: &Expr, visit: &mut dyn FnMut(&Expr)) {
    visit(expr);
    crate::walker::walk_expr_children(expr, &mut |child| walk_defines(child, visit));
}

/// Register the self-binding for `source_name` in the current scope. It stays
/// active while the class body lowers and is removed by
/// [`pop_decl_self_binding`].
pub(super) fn push_decl_self_binding(
    ctx: &mut LoweringContext,
    source_name: &str,
    template_name: &str,
) -> LocalId {
    let id = ctx.define_local(
        format!("__perry_class_decl_self_{template_name}"),
        crate::types::Type::Any,
    );
    ctx.class_expr_self_bindings
        .push((source_name.to_string(), ctx.scope_depth, id));
    ctx.class_decl_self_binding_ids.push(id);
    id
}

pub(super) fn pop_decl_self_binding(ctx: &mut LoweringContext, id: LocalId) {
    if let Some(pos) = ctx
        .class_expr_self_bindings
        .iter()
        .rposition(|(_, _, binding)| *binding == id)
    {
        ctx.class_expr_self_bindings.truncate(pos);
    }
    if let Some(pos) = ctx
        .class_decl_self_binding_ids
        .iter()
        .rposition(|binding| *binding == id)
    {
        ctx.class_decl_self_binding_ids.truncate(pos);
    }
    ctx.class_decl_self_binding = Some(id);
}

/// `this` in a static field initializer is the class being defined. For a
/// per-evaluation class that is the self-binding local, not the template.
/// `substitute_lexical_this_in_expr` adds the local to the capture list of
/// every `this`-capturing arrow it rewrites.
pub(super) fn substitute_static_this_with_self(expr: &mut Expr, self_id: LocalId) {
    crate::analysis::substitute_lexical_this_in_expr(expr, &Expr::LocalGet(self_id));
}

/// #11142: the self-binding local that `name` reads inside the body of a
/// per-evaluation class DECLARATION, when that is the nearest binding of
/// `name`. `new C()` constructs this evaluation and `x instanceof C` tests
/// against it, instead of using the shared template. A named class
/// expression's own binding is left alone (`None`): routing its `new` through
/// the local would mark the binding used and move an otherwise shared-template
/// class expression onto the fresh path.
pub(crate) fn fresh_class_decl_self_binding(ctx: &LoweringContext, name: &str) -> Option<LocalId> {
    ctx.resolve_class_self_binding(name)
        .filter(|id| ctx.class_decl_self_binding_ids.contains(id))
}
