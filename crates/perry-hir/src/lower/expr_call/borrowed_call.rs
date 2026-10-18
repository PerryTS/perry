//! A fresh, ordinary function literal has no own call property. Prove its
//! Function.prototype method before arguments run, retaining the one source
//! function and the complete original call on a failed proof.
use super::*;
use swc_ecma_visit::{Visit, VisitWith};

#[derive(Default)]
struct Definitions(bool);
impl Visit for Definitions {
    fn visit_function(&mut self, _: &ast::Function) {
        self.0 = true;
    }
    fn visit_arrow_expr(&mut self, _: &ast::ArrowExpr) {
        self.0 = true;
    }
    fn visit_class_expr(&mut self, _: &ast::ClassExpr) {
        self.0 = true;
    }
    fn visit_await_expr(&mut self, _: &ast::AwaitExpr) {
        self.0 = true;
    }
    fn visit_yield_expr(&mut self, _: &ast::YieldExpr) {
        self.0 = true;
    }
}

pub(super) fn borrowed_call_folds(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Result<Option<Expr>> {
    let ast::Callee::Expr(callee) = &call.callee else {
        return Ok(None);
    };
    let ast::Expr::Member(outer) = callee.as_ref() else {
        return Ok(None);
    };
    let ast::MemberProp::Ident(prop) = &outer.prop else {
        return Ok(None);
    };
    let method = prop.sym.as_ref();
    if !matches!(method, "call" | "apply") {
        return Ok(None);
    }
    if let Some(expr) = reflective_object_borrow(ctx, call, outer, method)? {
        return Ok(Some(expr));
    }
    if has_spread || method != "call" {
        return Ok(None);
    }
    let mut source = outer.obj.as_ref();
    while let ast::Expr::Paren(p) = source {
        source = &p.expr;
    }
    // Kind prototypes can independently override call. The runtime's checked
    // method entry proves those chains; keep their original reflective call.
    match source {
        ast::Expr::Fn(f) if !f.function.is_async && !f.function.is_generator => {}
        ast::Expr::Arrow(f) if !f.is_async => {}
        _ => return Ok(None),
    }
    let mut defs = Definitions::default();
    for arg in &call.args {
        arg.visit_with(&mut defs);
    }
    // A definition or suspension must not be duplicated across the two arms.
    if defs.0 {
        return Ok(None);
    }
    let function = lower_expr(ctx, source)?;
    let drops_this = matches!(&function,
        Expr::Closure { captures_this: false, is_arrow, body, .. }
            if *is_arrow || !crate::analysis::closure_uses_this(body));
    let args = call
        .args
        .iter()
        .map(|a| lower_expr(ctx, &a.expr))
        .collect::<Result<Vec<_>>>()?;
    let id = ctx.fresh_local();
    // Constant Function/eval specialization can introduce a definition that
    // was not visible to the AST visitor. Keep those lowered arguments once.
    if !drops_this || !patched_builtin_call::duplicable(&args) {
        let generic = patched_builtin_call::dynamic_member_call(
            call,
            Expr::LocalGet(id),
            method.to_string(),
            args,
        );
        return Ok(Some(Expr::ScopedTemp {
            id,
            value: Box::new(function),
            body: Box::new(generic),
        }));
    }
    let mut fast = Expr::Call {
        callee: Box::new(Expr::LocalGet(id)),
        args: args.iter().skip(1).cloned().collect(),
        type_args: Vec::new(),
        byte_offset: call.span.lo.0,
    };
    if let Some(this_arg) = args.first() {
        fast = Expr::Sequence(vec![this_arg.clone(), fast]);
    }
    let generic = patched_builtin_call::dynamic_member_call(
        call,
        Expr::LocalGet(id),
        method.to_string(),
        args,
    );
    Ok(Some(Expr::ScopedTemp {
        id,
        value: Box::new(function),
        body: Box::new(Expr::Conditional {
            condition: Box::new(Expr::NativeMethodCall {
                module: "__perry_borrowed_proof".to_string(),
                class_name: Some("Function.prototype".to_string()),
                object: Some(Box::new(Expr::LocalGet(id))),
                method: method.to_string(),
                args: Vec::new(),
            }),
            then_expr: Box::new(fast),
            else_expr: Box::new(generic),
        }),
    }))
}

// Read inherited Object-constructor methods through the real global object,
// bypassing the legacy GlobalGet(0) static-function value shortcut. This is a
// complete reflective call; no builtin identity is presumed.
fn reflective_object_borrow(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    outer: &ast::MemberExpr,
    method: &str,
) -> Result<Option<Expr>> {
    let ast::Expr::Member(source) = outer.obj.as_ref() else {
        return Ok(None);
    };
    let ast::Expr::Ident(base) = source.obj.as_ref() else {
        return Ok(None);
    };
    let ast::MemberProp::Ident(name) = &source.prop else {
        return Ok(None);
    };
    if base.sym != "Object"
        || !matches!(name.sym.as_ref(), "hasOwnProperty" | "propertyIsEnumerable")
        || ctx.lookup_local("Object").is_some()
        || ctx.lookup_func("Object").is_some()
        || ctx.lookup_class("Object").is_some()
        || ctx.lookup_imported_func("Object").is_some()
        || ctx.lookup_builtin_module_alias("Object").is_some()
        || ctx.lookup_native_module("Object").is_some()
        || !ctx.active_with_envs_for_ident("Object").is_empty()
    {
        return Ok(None);
    }
    let source = Expr::PropertyGet {
        byte_offset: 0,
        object: Box::new(Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(Expr::GlobalThisExpr),
            property: "Object".to_string(),
        }),
        property: name.sym.to_string(),
    };
    let args = call
        .args
        .iter()
        .map(|a| lower_expr(ctx, &a.expr))
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(patched_builtin_call::dynamic_member_call(
        call,
        source,
        method.to_string(),
        args,
    )))
}

#[cfg(test)]
mod tests {
    fn lowered(src: &str) -> String {
        let parsed = perry_parser::parse_typescript(src, "borrowed.ts").unwrap();
        format!(
            "{:?}",
            crate::lower_module(&parsed, "test", "borrowed.ts").unwrap()
        )
    }
    #[test]
    fn checked_iife_has_one_function_identity_and_the_original_call() {
        let hir = lowered("(function () { return 1; }).call(null);");
        assert_eq!(hir.matches("Closure {").count(), 1, "{hir}");
        assert!(
            hir.contains("ScopedTemp") && hir.contains("__perry_borrowed_proof"),
            "{hir}"
        );
        assert!(hir.contains("property: \"call\""), "{hir}");
    }
    #[test]
    fn captures_and_definitions_keep_their_actual_callable() {
        let hir = lowered("const m = [].slice; m.call([1]);");
        assert!(!hir.contains("ArrayLikeMethod"), "{hir}");
        let hir = lowered("Array.prototype.map.call([1], x => x);");
        assert_eq!(hir.matches("Closure {").count(), 1, "{hir}");
        assert!(!hir.contains("__perry_borrowed_proof"), "{hir}");
    }
}
