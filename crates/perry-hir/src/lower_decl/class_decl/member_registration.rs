//! Computed and non-computed class-member helpers, split out of
//! `class_decl.rs` to keep it under the 2000-line file gate.
//! Behaviour is unchanged; `use super::*` reaches the shared imports.

use super::*;

pub(super) fn lower_noncomputed_class_member_registration(
    ctx: &mut LoweringContext,
    method: &ast::ClassMethod,
    prop_name: &str,
    source_order: usize,
) -> Result<ClassComputedMember> {
    let function_name = noncomputed_member_registration_name(method.kind, method);
    let (kind, function) = match method.kind {
        ast::MethodKind::Method => (
            ClassComputedMemberKind::Method,
            with_static_member_context(ctx, method.is_static, |ctx| {
                lower_class_method_with_name(ctx, method, function_name)
            })?,
        ),
        ast::MethodKind::Getter => (
            ClassComputedMemberKind::Getter,
            with_static_member_context(ctx, method.is_static, |ctx| {
                lower_getter_method_with_name(ctx, method, function_name)
            })?,
        ),
        ast::MethodKind::Setter => (
            ClassComputedMemberKind::Setter,
            with_static_member_context(ctx, method.is_static, |ctx| {
                lower_setter_method_with_name(ctx, method, function_name)
            })?,
        ),
    };
    Ok(ClassComputedMember {
        key_expr: Expr::String(prop_name.to_string()),
        function,
        is_static: method.is_static,
        kind,
        source_order,
    })
}

/// Lower a generator `*[Symbol.iterator]()` class method (already lowered into
/// `func`, named `@@iterator`) into the runtime `@@iterator` vtable entry.
///
/// The body is lifted to a top-level `__perry_iter_<class>` generator with
/// `this` as an explicit first parameter — the generator transform (which only
/// visits `module.functions`) then rewrites it to the `{next, return, throw}`
/// closure triple, and the syntactic `for…of` fast path dispatches to it
/// directly via `iterator_func_for_class`.
///
/// But every *runtime*-dispatched iterator consumer (spread `[...x]`,
/// `Math.max(...x)`, destructuring, `x[Symbol.iterator]()`, `Array.from`)
/// resolves `@@iterator` through the class registry instead. So this also
/// returns a synthetic NON-generator `@@iterator` wrapper method that forwards
/// to the lifted generator (`return __perry_iter_X(this)`) for the caller to
/// append to the instance vtable. Without it the class carries no `@@iterator`
/// for those consumers to find and they throw "value is not iterable" (#5128).
/// (The runtime maps the well-known `Symbol.iterator` to this `@@iterator`
/// method name in `js_object_get_symbol_property`.)
///
/// Shared by `lower_class_decl` and `lower_class_from_ast` so class
/// declarations and class expressions behave identically.
pub(super) fn synthesize_symbol_iterator_wrapper(
    ctx: &mut LoweringContext,
    class_name: &str,
    func: &mut Function,
) -> Function {
    let this_id = ctx.fresh_local();
    let mut new_params = Vec::with_capacity(func.params.len() + 1);
    new_params.push(Param {
        id: this_id,
        name: "this".to_string(),
        ty: Type::Named(class_name.to_string()),
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    });
    new_params.append(&mut func.params);

    let mut body = std::mem::take(&mut func.body);
    crate::analysis::replace_this_in_stmts(&mut body, this_id);

    let top_fn_id = ctx.fresh_func();
    let top_fn = Function {
        id: top_fn_id,
        name: format!("__perry_iter_{}", class_name),
        type_params: Vec::new(),
        params: new_params,
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: true,
        is_strict: true,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    };
    ctx.pending_functions.push(top_fn);
    ctx.iterator_func_for_class
        .insert(class_name.to_string(), top_fn_id);

    Function {
        id: ctx.fresh_func(),
        name: "@@iterator".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::FuncRef(top_fn_id)),
            args: vec![Expr::This],
            type_args: Vec::new(),
            byte_offset: 0,
        }))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    }
}
