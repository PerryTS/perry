//! The `ast::Expr::Class` (class-expression-as-value) arm of `lower_expr_impl`,
//! extracted to a helper. Pure code move — no behavior change.

use super::*;
use crate::lower::*;
use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_common::Spanned;
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower_types::extract_ts_type_with_ctx;

pub(crate) fn lower_class_expr(
    ctx: &mut LoweringContext,
    class_expr: &ast::ClassExpr,
) -> Result<Expr> {
    let ident_name = class_expr.ident.as_ref().map(|i| i.sym.to_string());
    // A NAMED class EXPRESSION used as a VALUE whose name collides
    // with an existing module-scope class — a TOP-LEVEL `class X`
    // declaration OR an imported class binding — must NOT reuse that
    // class's name / ClassId. Per JS spec a class-expression's name
    // binds only inside its own body, so the two are distinct
    // classes. Reusing the id silently overwrote the real class with
    // the (often nearly empty) nested expression. minimatch's
    // `defaults()` returns
    //   `Object.assign(m, { Minimatch: class Minimatch extends
    //      orig.Minimatch {…}, AST: class AST extends orig.AST {…} })`
    // — `Minimatch` collides with the top-level `export class
    // Minimatch` (caught via `module_class_decl_names`), and `AST`
    // collides with the IMPORTED `import { AST } from './ast.js'`
    // (caught via `lookup_class`, since named class imports are
    // registered too). Both nested expressions hijacked the real
    // class id: `new Minimatch(pattern)` built a body-less instance,
    // and `AST.fromGlob(...)` inside `Minimatch.parse` dispatched to
    // the wrong (empty) class. Rename the colliding expression to a
    // fresh unique name so it gets its own ClassId; the value
    // position (object property / `new` site) holds the resulting
    // ClassRef directly, so the original name is not needed at module
    // scope. The `current_class` guard avoids renaming the rare
    // self-referential `class C { … new C() … }` expression form.
    let ident_name = match ident_name {
        Some(n)
            if (ctx.module_class_decl_names.contains(&n)
                || ctx.lookup_class(&n).is_some()
                || ctx.lookup_imported_func(&n).is_some())
                && ctx.current_class.as_deref() != Some(n.as_str()) =>
        {
            Some(format!("{}__class_expr_{}", n, ctx.fresh_class()))
        }
        other => other,
    };
    // When the HIR registration key we pick below diverges from the
    // class's user-visible `.name`, record the real name here so codegen
    // registers it instead of the synthetic key (#5592).
    let mut display_override: Option<String> = None;
    let synthetic_name = match ident_name {
        Some(n) => n,
        None => {
            let inferred = if !anonymous_class_has_static_name_member(&class_expr.class) {
                ctx.assignment_inferred_name
                    .as_ref()
                    .filter(|name| !name.is_empty())
                    .cloned()
            } else {
                None
            };
            match inferred {
                // First class expression to claim this inferred binding name —
                // reuse it directly as the registration key (and thus `.name`).
                Some(name) if ctx.lookup_class(&name).is_none() => name,
                // #5592: a second anonymous class expression assigned to the
                // SAME binding (`C = class {…}; C = class {…}`) infers the same
                // name. Reusing the key would alias both onto one ClassId
                // (`lower_class_from_ast` dedups by name via `lookup_class`),
                // silently dropping the second body. Give it a fresh, unique
                // registration key but keep its user-visible `.name` as the
                // binding name.
                Some(name) => {
                    display_override = Some(name.clone());
                    format!("{}__anon_dup_{}", name, ctx.fresh_class())
                }
                None => format!("__anon_class_{}", ctx.fresh_class()),
            }
        }
    };
    let class = lower_class_from_ast(ctx, &class_expr.class, &synthetic_name, false)?;
    if let Some(display) = display_override {
        ctx.class_display_names.insert(class.id, display);
    }
    // Mixin factories like `function WithA(B) { return class extends B {} }`
    // produce a class whose super is the function-parameter `B` — a
    // runtime value, not a statically-known class. The class-decl arm
    // at the top of this file only pushes a `RegisterClassParentDynamic`
    // statement for top-level class declarations; an anonymous class
    // expression inside a function body never has that side effect
    // fire, so `new (class extends WithA(Base) {})().baseMethod()`
    // walks subclass → inner factory class and stops at the unwired
    // grandparent edge (TypeError on the inherited method). Sequence
    // the dynamic-parent registration in front of the ClassRef so the
    // edge is wired every time the factory function executes; the
    // Sequence yields its last element, so the value remains the
    // ClassRef the call site expects.
    let parent_expr = class.extends_expr.clone();
    // Issue #894: collect computed-Symbol-key static fields so
    // codegen emits a `RegisterClassStaticSymbol` registration
    // sequenced in front of the ClassRef. Without this, the
    // registration happens at module init via
    // `init_static_fields_late` — but the values referenced by
    // the key/init may not be valid yet (the factory hasn't been
    // called, so any function-local captures are zero) or the
    // class lookup may happen BEFORE module init's late phase
    // (within the same module's top-level expressions). Effect's
    // `make()` factory's `static [TypeId] = variance` is the
    // canonical case: `isSchema(C)` was called from Schema.ts's
    // own top-level `class extends transform(...)` chains, which
    // run before the module's `init_static_fields_late`.
    let static_symbol_registrations: Vec<(Expr, Expr)> = class
        .static_fields
        .iter()
        .filter_map(|sf| match (sf.key_expr.as_ref(), sf.init.as_ref()) {
            (Some(k), Some(v)) => Some((k.clone(), v.clone())),
            _ => None,
        })
        .collect();
    // Issue #1772: regular-named static fields with an initializer
    // (`static ast = ast`). #894 only handled the Symbol-key case;
    // these need the same per-evaluation treatment, otherwise a class
    // expression returned from a factory (effect's `make`) shares one
    // template class and `.ast` is undefined/clobbered.
    let named_statics: Vec<(String, Expr)> = class
        .static_fields
        .iter()
        .filter_map(|sf| match (sf.key_expr.as_ref(), sf.init.as_ref()) {
            (None, Some(v)) => Some((sf.name.clone(), v.clone())),
            _ => None,
        })
        .collect();
    let computed_member_registrations: Vec<Expr> = class
        .computed_members
        .iter()
        .map(|member| class_computed_member_registration_expr(&synthetic_name, member))
        .collect();
    let captured_args: Vec<Expr> = ctx
        .lookup_class_captures(&synthetic_name)
        .map(|ids| ids.iter().map(|id| Expr::LocalGet(*id)).collect())
        .unwrap_or_default();
    // Static block synthetic-method names (`__perry_static_init_N`), in
    // source order — emitted as inline `StaticMethodCall`s on the
    // shared-template path so blocks run at class-evaluation time (the
    // same treatment the class-declaration path gives them).
    let static_block_names: Vec<String> = class
        .static_methods
        .iter()
        .filter(|m| m.name.starts_with("__perry_static_init_"))
        .map(|m| m.name.clone())
        .collect();
    ctx.pending_classes.push(class);
    // #1772: a class EXPRESSION that carries per-evaluation static
    // fields and is NOT a mixin (`class extends <expr>`) lowers to a
    // fresh heap class object per evaluation (`ClassExprFresh`), so
    // `make(a) !== make(b)` and each holds its own statics as own
    // properties. Mixins and class expressions without statics/captures
    // keep the historical (shared-template) path.
    // A class expression evaluated at module top level runs exactly
    // once, so it needs no per-evaluation freshness — route it through
    // the shared-template `ClassRef` path (identical to a class
    // declaration), where static field/element initializers run via
    // `init_static_fields_late` and a static method's `this` resolves
    // to the class-ref. The `ClassExprFresh` path is reserved for class
    // expressions inside a function body (factories like effect's
    // `make()`), which produce a distinct class object per call.
    let at_module_top = ctx.scope_depth == 0 && ctx.inside_block_scope == 0;
    if !at_module_top
        && parent_expr.is_none()
        && (!named_statics.is_empty()
            || !static_symbol_registrations.is_empty()
            || !captured_args.is_empty())
    {
        // #1787: snapshot the class's captured outer-scope values so a
        // later `new <classObjectValue>()` can run the instance-field
        // initializers / constructor body with the right environment.
        // `synthesize_class_captures` (run during `lower_class_from_ast`
        // above) appended one `__perry_cap_<id>` constructor param per
        // captured outer id, in `captures_vec` order — read them back in
        // that same order as `LocalGet(outer_id)`, evaluated here where
        // the captures are still live.
        let fresh_expr = Expr::ClassExprFresh {
            template: synthetic_name,
            named_statics,
            symbol_statics: static_symbol_registrations,
            captured_args,
        };
        if computed_member_registrations.is_empty() {
            return Ok(fresh_expr);
        }
        let mut seq = computed_member_registrations;
        seq.push(fresh_expr);
        return Ok(Expr::Sequence(seq));
    }
    let mut seq: Vec<Expr> = Vec::new();
    if let Some(p) = parent_expr {
        seq.push(Expr::RegisterClassParentDynamic {
            class_name: synthetic_name.clone(),
            parent_expr: p,
        });
    }
    // #5437 (p-queue PQueue undefined-`.default` capture): a class EXPRESSION
    // that captures enclosing-scope locals AND reaches the shared-template
    // (`ClassRef`) path — i.e. one with heritage (`class extends t { … uses
    // n … }`) or evaluated at module top — must snapshot its decl-site capture
    // values, exactly like the class-DECLARATION path does
    // (`lower_decl/body_stmt.rs`). The `ClassExprFresh` path above carries
    // captures via `captured_args` at construction, but the shared-template
    // path produces a stable `ClassRef` constructed later through the runtime
    // construct path (`construct_registered_class_ref` →
    // `replay_registered_class_constructor`), which fills the synthesized
    // `__perry_cap_*` ctor params SOLELY from `CLASS_CAPTURE_VALUES`. Without a
    // registered snapshot those params arrive `undefined`. The Next.js route
    // bundle's p-queue `PQueue` (`c.default = class extends t { … queueClass:
    // n.default … }`, instantiated via `new (tH())()` → the runtime construct
    // path) read its captured module ref `n` (idx 1) as `undefined` and threw
    // `Cannot read properties of undefined (reading 'default')`. Emitting the
    // snapshot here mirrors the class-decl path's `RegisterClassCaptures` and
    // closes the gap for every heritage/module-top capturing class expression.
    //
    // Known limitation (matches the class-DECLARATION path): the snapshot is
    // keyed by `synthetic_name`, which is stable per source location, so it
    // occupies a single `CLASS_CAPTURE_VALUES` slot. A heritage class
    // expression re-evaluated with different captures (e.g. inside a function
    // called more than once) overwrites the previous snapshot; a `ClassRef`
    // from an earlier evaluation that is *constructed* after a later evaluation
    // would observe the newer capture values. The shared-template `ClassRef`
    // mechanism is name-keyed by design, so this is not made per-evaluation
    // here — the captured-at-construction `ClassExprFresh` path above is the
    // per-instance route. In practice the snapshot is written immediately
    // before the class is registered/constructed, so the common case (build
    // then construct, including the p-queue `PQueue` repro) is unaffected.
    if !captured_args.is_empty() {
        seq.push(Expr::RegisterClassCaptures {
            class_name: synthetic_name.clone(),
            captures: captured_args.clone(),
        });
    }
    seq.extend(computed_member_registrations);
    for (k, v) in static_symbol_registrations {
        seq.push(Expr::RegisterClassStaticSymbol {
            class_name: synthetic_name.clone(),
            key_expr: Box::new(k),
            value_expr: Box::new(v),
        });
    }
    // Inline the named static field/element initializers at the point
    // the class expression evaluates (source order), mirroring the
    // class-declaration path. Without this the shared-template path
    // relied solely on the late `init_static_fields_late` pass, which
    // runs AFTER the surrounding top-level statements — so a read like
    // `C.x` immediately after `var C = class { static x = 1 }` saw the
    // uninitialized (0.0) slot. (Private statics carry a `#`-prefixed
    // name and flow through the same StaticFieldSet path.)
    for (name, v) in named_statics {
        seq.push(Expr::StaticFieldSet {
            class_name: synthetic_name.clone(),
            field_name: name,
            value: Box::new(v),
        });
    }
    // Static blocks run right after the static-field initializers, in
    // source order, with the class as `this`.
    for block_name in static_block_names {
        seq.push(Expr::StaticMethodCall {
            class_name: synthetic_name.clone(),
            method_name: block_name,
            args: Vec::new(),
        });
    }
    if seq.is_empty() {
        Ok(Expr::ClassRef(synthetic_name))
    } else {
        seq.push(Expr::ClassRef(synthetic_name));
        Ok(Expr::Sequence(seq))
    }
}
