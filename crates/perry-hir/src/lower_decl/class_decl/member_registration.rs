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

/// Register a generator `*[Symbol.iterator]()` instance method (already
/// lowered into `func`) and return the function to install under the computed
/// `Symbol.iterator` key.
///
/// The generator is installed AS the method, exactly like any other
/// computed-key generator method (`*[K]()`), so its body keeps the ordinary
/// method receiver: `this`, `#private` brand guards, arrows that capture
/// `this`, and nested private-method calls all resolve against the instance.
/// `js_register_class_computed_method` aliases the well-known symbol onto the
/// `@@iterator` vtable slot, which every runtime-dispatched consumer (spread
/// `[...x]`, destructuring, `Array.from`, `x[Symbol.iterator]()`, `for…of`
/// through `GetIterator`) resolves (#5128, #9788).
///
/// This used to lift the body into a top-level `__perry_iter_<class>`
/// generator with `this` rewritten to an explicit first parameter by
/// `replace_this_in_stmts`, plus a forwarding `@@iterator` wrapper. That
/// rewrite is a hand-written walker that misses most expression shapes: it
/// never reached a `PrivateGuard` / `PrivateBrandCheck` receiver (so
/// `this.#head` threw "Cannot access private member from an object whose
/// class did not declare it" — @redis/client's linked lists, #11170), and it
/// skipped arrow bodies, template literals and several statement kinds, so
/// their `this` read `undefined`. Nothing calls the lifted function directly
/// any more (the `for…of` fast path goes through `GetIterator` since #9788),
/// so the lift bought nothing.
///
/// `iterator_func_for_class` still records the class so the syntactic
/// `for (… of new C())` detection keeps taking the iterator-protocol loop.
///
/// Shared by `lower_class_decl` and `lower_class_from_ast` so class
/// declarations and class expressions behave identically.
pub(super) fn register_symbol_iterator_generator(
    ctx: &mut LoweringContext,
    class_name: &str,
    func: Function,
) -> Function {
    ctx.iterator_func_for_class
        .insert(class_name.to_string(), func.id);
    func
}
