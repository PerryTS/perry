use super::*;

/// Class heritage is evaluated inside the class's lexical name binding. That
/// binding is uninitialized until the heritage expression finishes, so
/// `class C extends C {}` (including a parenthesized `C`) must fail with a
/// ReferenceError instead of resolving an outer binding or creating a
/// recursive static parent edge.
pub(super) fn is_class_self_heritage(expr: &ast::Expr, inner_name: &str) -> bool {
    match expr {
        ast::Expr::Ident(ident) => ident.sym == inner_name,
        ast::Expr::Paren(paren) => is_class_self_heritage(&paren.expr, inner_name),
        _ => false,
    }
}

/// Class definitions are strict-mode code, including function expressions
/// created while evaluating the heritage. Keep the strict context scoped to
/// the heritage expression so a superclass such as
/// `class D extends function(){ arguments.callee } {}` gets a strict
/// arguments object without leaking strictness into surrounding source.
pub(super) fn lower_class_heritage_expr(
    ctx: &mut LoweringContext,
    expr: &ast::Expr,
) -> Result<Expr> {
    ctx.enter_strict_mode(true);
    let lowered = lower_expr(ctx, expr);
    ctx.exit_strict_mode();
    lowered
}

/// Whether this class delegates directly to a dynamic-function heritage.
/// `true` means its implicit constructor forwards the `new` site's arguments;
/// `false` is the exact no-argument `constructor() { super(); }` form.
pub(super) fn dynamic_function_forwarding_mode(class: &ast::Class) -> Option<bool> {
    let constructor = class.body.iter().find_map(|member| match member {
        ast::ClassMember::Constructor(constructor) => Some(constructor),
        _ => None,
    });
    let Some(constructor) = constructor else {
        return Some(true);
    };
    if !constructor.params.is_empty() {
        return None;
    }
    let [ast::Stmt::Expr(statement)] = constructor.body.as_ref()?.stmts.as_slice() else {
        return None;
    };
    let ast::Expr::Call(call) = statement.expr.as_ref() else {
        return None;
    };
    if matches!(call.callee, ast::Callee::Super(_)) && call.args.is_empty() {
        Some(false)
    } else {
        None
    }
}

/// JS global constructors whose `super(...)` codegen special-cases by the
/// parent's textual NAME: it installs the built-in's state or constructs a
/// branded exotic value instead of running the parent's own constructor. Keep
/// in lockstep with `is_other_builtin_constructor_name` in
/// `perry-codegen/src/expr/this_super_call.rs`.
fn is_name_keyed_global_builtin_ctor(name: &str) -> bool {
    matches!(
        name,
        "Map"
            | "Set"
            | "WeakMap"
            | "WeakSet"
            | "EventTarget"
            | "Array"
            | "ArrayBuffer"
            | "SharedArrayBuffer"
            | "DataView"
            | "Boolean"
            | "Number"
            | "String"
            | "Date"
            | "RegExp"
            | "URL"
            | "Promise"
            | "Function"
            | "BigInt"
            | "Symbol"
            | "Object"
            | "Int8Array"
            | "Uint8Array"
            | "Uint8ClampedArray"
            | "Int16Array"
            | "Uint16Array"
            | "Int32Array"
            | "Uint32Array"
            | "Float32Array"
            | "Float64Array"
            | "BigInt64Array"
            | "BigUint64Array"
    )
}

/// #11139: `class X extends ns.URL {}` names a PROPERTY of `ns`, not the global
/// `URL`. The member arms keep only the trailing property as `extends_name`,
/// and codegen keys its built-in `super()` routes on that bare name, so a
/// package class that merely happens to be called `URL` (whatwg-url's, reached
/// as `whatwg_url_1.URL` from mongodb-connection-string-url) was constructed
/// as Perry's native URL. The package constructor never ran, so its brand
/// check rejected the instance. Returns true when the static name must be
/// dropped so the parent resolves only through `extends_expr`.
///
/// The name still means the built-in when the object is a global-object alias
/// (`globalThis.URL`) or a native-module binding (`url.URL` from
/// `import * as url from "url"`), so those keep it. The static `extends`
/// link is dropped with the name: a trailing-property lookup can only ever
/// guess which class the member holds, and the dynamic parent path is the one
/// every other named member heritage already relies on.
pub(super) fn member_heritage_hides_global_builtin(
    ctx: &LoweringContext,
    member: &ast::MemberExpr,
    parent_name: &str,
) -> bool {
    if !is_name_keyed_global_builtin_ctor(parent_name) {
        return false;
    }
    let mut obj = member.obj.as_ref();
    while let ast::Expr::Paren(paren) = obj {
        obj = paren.expr.as_ref();
    }
    match obj {
        ast::Expr::Ident(ident) => {
            let n = ident.sym.as_ref();
            let global_alias = matches!(n, "globalThis" | "global" | "window" | "self")
                && ctx.locals.lookup(n).is_none();
            !global_alias && ctx.lookup_native_module(n).is_none()
        }
        _ => true,
    }
}
