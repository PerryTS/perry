//! Forward-captured class declarations whose binding is a per-evaluation class
//! value.
//!
//! A function-nested `class C extends Array { … }` (any heritage the compiler
//! cannot resolve to a known class, private elements, or computed field keys)
//! is lowered to `let C = ClassExprFresh { … }`: the binding holds THIS
//! evaluation's class object, and the static template `ClassRef(C)` is a
//! different object with its own prototype. A closure created EARLIER in the
//! body than the declaration (`var go = () => new C(n)` above `class C`, the
//! UMD/webpack factory shape of fuzzysort's `KeysResult`) used to resolve `C`
//! to the template, so its instances missed every method of the evaluated
//! class and failed `instanceof C`. JS resolves such a reference through the
//! class's lexical binding at call time; these helpers pre-register that
//! binding as a boxed TDZ local exactly like a forward-captured `let`, and the
//! declaration initializes it (`body_stmt.rs`).

use std::collections::HashSet;

use swc_ecma_ast as ast;

use crate::lower::LoweringContext;

/// Will this class declaration be lowered to a per-evaluation class value
/// (`ClassExprFresh`)? Decided from the declaration's own syntax and the
/// scope's bindings before the body is lowered. It may over-approximate: a
/// pre-registered binding whose class turns out to share the template is
/// initialized with that template `ClassRef`, which is the same value.
/// `scope_classes` names every class declared in the same scope and
/// `evaluated_siblings` the earlier ones this predicate accepted: a subclass of
/// a sibling class is per-evaluation exactly when its parent is.
pub(crate) fn class_decl_binds_evaluated_class(
    ctx: &LoweringContext,
    class_decl: &ast::ClassDecl,
    scope_classes: &HashSet<String>,
    evaluated_siblings: &HashSet<String>,
) -> bool {
    let class = &class_decl.class;
    let private_or_computed_field = class.body.iter().any(|member| match member {
        ast::ClassMember::PrivateMethod(_) | ast::ClassMember::PrivateProp(_) => true,
        ast::ClassMember::AutoAccessor(accessor) => {
            matches!(accessor.key, ast::Key::Private(_))
        }
        ast::ClassMember::ClassProp(prop) => matches!(prop.key, ast::PropName::Computed(_)),
        _ => false,
    });
    if private_or_computed_field {
        return true;
    }
    let Some(parent) = class.super_class.as_deref() else {
        return false;
    };
    let ast::Expr::Ident(ident) = parent else {
        return true;
    };
    let name = ident.sym.as_ref();
    if evaluated_siblings.contains(name) {
        return true;
    }
    if scope_classes.contains(name) {
        return false;
    }
    if ctx.lookup_local(name).is_some() {
        return true;
    }
    if is_native_instance_parent_name(name) {
        return false;
    }
    let resolved = ctx.resolve_class_name(name);
    ctx.lookup_class(&resolved).is_none() && !ctx.forward_class_names.contains(name)
}

/// Parent names `lower_body_class_decl` binds as a native parent
/// (`native_extends`), which keeps the class on the shared template.
fn is_native_instance_parent_name(name: &str) -> bool {
    matches!(
        name,
        "EventEmitter"
            | "EventEmitterAsyncResource"
            | "AsyncLocalStorage"
            | "AsyncResource"
            | "WebSocketServer"
            | "ReadableStream"
            | "WritableStream"
            | "TransformStream"
            | "Readable"
            | "Writable"
            | "Duplex"
            | "Transform"
            | "PassThrough"
    )
}

/// The declaration site of a pre-registered class binding: take it out of
/// name resolution while the class itself is lowered, so the class body sees
/// exactly what it saw without the forward binding (its own name inside its
/// members is the class's inner binding). Returns the binding id, which the
/// caller then initializes.
pub(crate) fn take_forward_class_binding(
    ctx: &mut LoweringContext,
    class_decl: &ast::ClassDecl,
) -> Option<crate::types::LocalId> {
    let id = ctx
        .lexical_forward_decls
        .remove(&class_decl.ident.span.lo.0)?;
    let name = class_decl.ident.sym.as_ref();
    if let Some(index) = ctx.lookup_local_index(name) {
        if ctx.locals[index].1 == id {
            ctx.locals.remove(index);
        }
    }
    Some(id)
}
