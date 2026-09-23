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

use crate::ir::Expr;
use crate::lower::LoweringContext;
use crate::types::LocalId;

/// Syntactic private-element check: the class will report
/// `has_private_elements()` once lowered.
pub(super) fn class_body_has_private_names(class: &ast::Class) -> bool {
    class.body.iter().any(|member| match member {
        ast::ClassMember::PrivateMethod(_) | ast::ClassMember::PrivateProp(_) => true,
        ast::ClassMember::AutoAccessor(accessor) => {
            matches!(accessor.key, ast::Key::Private(_))
        }
        _ => false,
    })
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
    ctx.class_decl_self_binding = Some(id);
}

/// `this` in a static field initializer is the class being defined. For a
/// per-evaluation class that is the self-binding local, not the template.
/// `substitute_lexical_this_in_expr` adds the local to the capture list of
/// every `this`-capturing arrow it rewrites.
pub(super) fn substitute_static_this_with_self(expr: &mut Expr, self_id: LocalId) {
    crate::analysis::substitute_lexical_this_in_expr(expr, &Expr::LocalGet(self_id));
}
