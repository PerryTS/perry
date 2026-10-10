//! Shared lowering of a direct class-expression variable declarator.
//!
//! Export syntax changes the visibility of the binding, not NamedEvaluation
//! or the class identity used by forward references to that binding.

use super::*;

pub(crate) fn lower_class_expression_declarator(
    ctx: &mut LoweringContext,
    module: &mut Module,
    decl: &ast::VarDeclarator,
    mutable: bool,
    is_var: bool,
) -> Result<bool> {
    let (ast::Pat::Ident(ident), Some(init)) = (&decl.name, &decl.init) else {
        return Ok(false);
    };
    let mut inner_expr = init.as_ref();
    loop {
        inner_expr = match inner_expr {
            ast::Expr::Paren(p) => &p.expr,
            ast::Expr::TsAs(a) => &a.expr,
            ast::Expr::TsNonNull(n) => &n.expr,
            ast::Expr::TsTypeAssertion(a) => &a.expr,
            _ => break,
        };
    }
    let ast::Expr::Class(class_expr) = inner_expr else {
        return Ok(false);
    };
    let bind_name = ident.id.sym.to_string();
    // A registry collision uses the general expression path, which assigns
    // a distinct template key and preserves the evaluated binding value.
    if ctx.lookup_class(&bind_name).is_none() {
        // `var X = class _X { ... new _X() ... }` gives `_X`
        // a lexical binding only inside the class body. Allocate
        // the class id under the outer binding here; the lowering
        // context's `current_class_inner_name` handling resolves
        // `_X` while lowering methods and initializers. Registering
        // `_X` globally as an alias leaks it outside the class
        // expression (`typeof _X` must remain `"undefined"`).
        let inner_name_for_register = class_expr
            .ident
            .as_ref()
            .map(|i| i.sym.to_string())
            .filter(|n| n != &bind_name);
        if inner_name_for_register.is_some() {
            // Allocate the class id eagerly so
            // lower_class_from_ast picks up the same id via
            // lookup_class(bind_name).
            let class_id = ctx.fresh_class();
            ctx.register_class(bind_name.clone(), class_id);
            // The bind-name local holds ITS OWN
            // class (see inferred_class_bindings).
            ctx.inferred_class_bindings.insert(bind_name.clone());
        }
        // The inner (const) binding visible inside the
        // body is the class expression's own source ident
        // (`var cls = class C {...}` -> `C`); feed it so the
        // const-assignment guard uses the user-visible name,
        // not the `bind_name` registration key.
        ctx.pending_class_inner_name = class_expr.ident.as_ref().map(|i| i.sym.to_string());
        // Lower the class with the binding name so
        // `new BindName(...)` works unchanged.
        let lowered_class =
            crate::lower_decl::lower_class_from_ast(ctx, &class_expr.class, &bind_name, false)?;
        if let Some(inner_name) = inner_name_for_register {
            // #6679: a NAMED class EXPRESSION's `.name`
            // is its own explicit name (`Named` in
            // `const B = class Named {}`), NOT the outer
            // binding name. Per spec a named class
            // expression is not an anonymous function
            // definition, so assignment NamedEvaluation
            // (`SetFunctionName` from `const B =`) must
            // not override the declared name. This fast
            // path registers the class under `bind_name`
            // so `new B()` / `instanceof B` resolve
            // statically, so record a display-name
            // override (the #5592 mechanism, exactly like
            // the mixin arm below) that codegen emits for
            // the `.name` string constant instead of the
            // `bind_name` registration key.
            ctx.class_display_names
                .insert(lowered_class.id, inner_name.clone());
        }
        // Computed member keys (`static get [expr]()`,
        // `[expr]() {}`) register at runtime against the
        // class id — the general class-expression arm in
        // `lower_expr.rs` sequences these in front of the
        // `ClassRef`. This `var C = class {…}` fast path
        // emits a bare `ClassRef` binding instead, so emit
        // the same registrations here or the computed
        // accessors/methods never reach the side tables
        // (Test262 accessor-name-{static,inst}/computed).
        let computed_member_registrations: Vec<Expr> = lowered_class
            .computed_members
            .iter()
            .map(|member| class_computed_member_registration_expr(&bind_name, member))
            .collect();
        // Runtime-value parent (`var X = class extends
        // <expr> {}` where the parent isn't a known class —
        // e.g. @hono/node-server's `var Request = class
        // extends GlobalRequest {}`, `GlobalRequest =
        // global.Request`). The general class-expression arm
        // in `lower_expr.rs` and the `Decl::Class` arms emit
        // `RegisterClassParentDynamic` so the parent edge —
        // and the fetch-parent kind for Request/Response
        // subclasses — is wired at module init (where the
        // alias still resolves). This `var C = class {…}`
        // fast path emitted a bare `ClassRef` binding and
        // skipped it, so the parent never registered and a
        // `Request`/`Response` subclass got no native handle
        // (inherited body methods threw "text is not a
        // function"). Emit it here too, in source order
        // before the value binding. Clone the extends
        // expression before `push_class_dedup` moves the
        // class out.
        let parent_register = lowered_class.extends_expr.clone().map(|p| {
            Stmt::Expr(Expr::RegisterClassParentDynamic {
                class_name: bind_name.clone(),
                parent_expr: p,
            })
        });
        // Inline static field/element initializers and
        // static blocks at the class-expression's source
        // position, exactly as the `Decl::Class` arm does
        // for declarations. Without this the `var C =
        // class { static x = 1 }` fast path relied solely
        // on the late `init_static_fields_late` codegen
        // pass, which runs AFTER the surrounding top-level
        // statements — so `C.x` read immediately after the
        // binding saw the uninitialized (0.0) slot, and a
        // static method's `this.#priv` read undefined.
        // Interleaved in source order (see
        // `build_interleaved_static_init_stmts`) rather
        // than all fields then all blocks.
        let static_init_stmts = crate::lower_decl::build_interleaved_static_init_stmts(
            &class_expr.class.body,
            &bind_name,
            &lowered_class.fields,
            &lowered_class.static_fields,
            &lowered_class.static_methods,
        );
        push_class_dedup(module, lowered_class);
        if let Some(reg) = parent_register {
            module.init.push(reg);
        }
        for reg in computed_member_registrations {
            module.init.push(Stmt::Expr(reg));
        }
        for s in static_init_stmts {
            module.init.push(s);
        }
        // Register the alias so `new X()` → `new X()`
        // (no-op lookup, but marks the binding as a class).
        ctx.class_expr_aliases
            .insert(bind_name.clone(), bind_name.clone());
        emit_class_expression_value_binding(ctx, module, &bind_name, mutable, is_var);
        return Ok(true);
    }
    Ok(false)
}
