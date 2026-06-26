//! ClassRef-receiver static-method dispatch tower (#687 / #915 / #1787 / #321).
//! Pure code move from `property_get.rs` — no behavior change.

use super::*;

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, nanbox_pointer_inline, nanbox_string_inline, unbox_to_i64, FnCtx};
use crate::lower_array_method::lower_array_method;
use crate::lower_string_method::{is_known_string_method_name, lower_string_method};
use crate::nanbox::double_literal;
use crate::type_analysis::{
    is_array_expr, is_global_constructor_expr, is_map_expr, is_native_module_dynamic_index,
    is_promise_expr, is_set_expr, is_string_expr, is_url_search_params_expr, receiver_class_name,
};
use crate::types::{DOUBLE, I32, I64};

/// Issue #687 — ClassRef receiver static-method dispatch.
/// `ClassName.method(args)` where `ClassName` lowered to `Expr::ClassRef` (an
/// INT32-NaN-boxed class id) rather than a pointer to an instance.
///
/// See the original narrative comments inline below for the full motivation
/// (Effect Schema's `BigIntFromSelf.pipe(...)`, factory-produced classes,
/// imported/namespace classes, static-field-holding callables, runtime
/// parent-chain dispatch). Returns `Ok(Some(_))` when a static receiver was
/// recognised and a result emitted; `Ok(None)` to continue the tower.
pub(crate) fn try_lower_static_dispatch(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    object: &Expr,
    property: &str,
    args: &[Expr],
) -> Result<Option<String>> {
    let static_dispatch_cls: Option<String> = resolve_static_dispatch_cls(
        object,
        &ctx.local_id_to_name,
        &ctx.local_class_aliases,
        ctx.func_returns_class,
        ctx.class_ids,
    );
    if let Some(cls_name) = static_dispatch_cls {
        // `C.prop(args)` where `prop` is a static ACCESSOR reads the accessor and
        // calls its result — handle before the by-name tower (which would miss).
        if let Some(v) = crate::lower_call::console_promise::try_lower_class_static_accessor_call(
            ctx, &cls_name, property, callee, args,
        )? {
            return Ok(Some(v));
        }
        // (fn_name, is_static, declared_param_count, has_rest, is_synthetic_arguments)
        let mut resolved: Option<(String, bool, usize, bool, bool)> = None;
        let mut cur = Some(cls_name.clone());
        while let Some(c) = cur {
            if let Some(class_info) = ctx.classes.get(&c) {
                let sm = class_info
                    .static_methods
                    .iter()
                    .find(|m| m.name == *property);
                if let Some(sm) = sm {
                    let key = (
                        c.clone(),
                        crate::codegen::static_method_registry_key(property),
                    );
                    if let Some(fname) = ctx.methods.get(&key).cloned() {
                        let declared = sm.params.len();
                        let has_rest = sm.params.last().map(|p| p.is_rest).unwrap_or(false);
                        let is_synth_args = sm
                            .params
                            .last()
                            .map(|p| p.arguments_object.is_some())
                            .unwrap_or(false);
                        resolved = Some((fname, true, declared, has_rest, is_synth_args));
                        break;
                    }
                }
            }
            cur = ctx
                .classes
                .get(&c.clone())
                .and_then(|cc| cc.extends_name.clone());
        }
        if let Some((fn_name, _is_static, declared, has_rest, is_synth_args)) = resolved {
            // Receiver-box selection (`this` inside the static body):
            //   - `ClassRef`: `lower_expr` already yields the
            //     INT32-NaN-boxed class id; `this === ClassRef`.
            //   - `Call` (factory return): `lower_expr` returns the
            //     dynamic class produced by the factory, so each
            //     `Literal(value)` / `make(ast)` call carries
            //     unique static fields (`static literals = […]`,
            //     `static ast = …`). The static body reads those
            //     through `this.<field>`, so passing the synthesized
            //     ClassRef would lose the per-call data — use the
            //     actual lowered call result instead.
            //   - Everything else (`LocalGet` after a
            //     `const Cls = make()` collapse, etc.): synthesize
            //     a fresh ClassRef NaN-box. The static body's
            //     `this.<field>` then dispatches through the
            //     ClassRef's class-keys + class-field side-table,
            //     which is the post-#912 (gap 2) shape.
            let recv_box = match object {
                Expr::ClassRef(_) => lower_expr(ctx, object)?,
                Expr::Call { .. } => lower_expr(ctx, object)?,
                Expr::Sequence(_) => lower_expr(ctx, object)?,
                // #1787: a class-expression value is a real heap class
                // object whose per-evaluation static fields are OWN
                // properties. Use the actual lowered object as `this` (NOT a
                // synthesized ClassRef) so `this.ast` inside the static body
                // reads this evaluation's own field rather than the shared
                // template's static-field global.
                Expr::ClassExprFresh { .. } => lower_expr(ctx, object)?,
                // #1787: `const C = make(...); C.staticMethod()`. The local
                // holds the class-expression's heap object (or, for a
                // top-level-class alias like `const F = Foo`, the same
                // INT32 ClassRef the synthesized fallback would produce).
                // Loading the actual stored value preserves the
                // per-evaluation own static fields a synthesized ClassRef
                // would discard, and is value-identical for the ClassRef
                // case — so `this.<field>` resolves correctly either way.
                Expr::LocalGet(_) => lower_expr(ctx, object)?,
                _ => {
                    // Synthesize a ClassRef NaN-box from the resolved class.
                    let cid = ctx.class_ids.get(&cls_name).copied().unwrap_or(0);
                    let bits = crate::nanbox::INT32_TAG | (cid as u64 & 0xFFFF_FFFF);
                    crate::nanbox::double_literal(f64::from_bits(bits))
                }
            };
            // Refs #915 (gap 3 / #321 follow-up): Effect's `class
            // SchemaClass { static pipe() { ... arguments ... } }`
            // factory returns an anon class whose `pipe` reads
            // `arguments.length` to dispatch. The HIR appends a
            // synthesized `arguments` rest param (#677 / #899). The
            // direct-call dispatch here previously forwarded the
            // call args 1:1 to the function whose only declared
            // parameter is the rest array — so for
            // `Cls.pipe(f1, f2)` the function got `arg0 = f1` (then
            // read .length = "function" → undefined). Mirror the
            // arg-bundling logic from the regular Call lowering
            // (lines ~720–765) so the rest slot receives a real
            // array of all call args, matching JS `arguments`
            // semantics. The non-synthetic rest path (e.g.
            // `static foo(a, ...rest)`) follows the same shape:
            // pass the first `declared-1` positional args as-is,
            // then bundle the trailing args into an Array.
            let mut lowered: Vec<String> = Vec::with_capacity(args.len());
            if has_rest && is_synth_args {
                // Lower each call arg exactly ONCE (a value may have side
                // effects), then reuse the SSA registers both for the leading
                // real params and for the synthesized `arguments` object.
                let mut vals: Vec<String> = Vec::with_capacity(args.len());
                for a in args {
                    vals.push(lower_expr(ctx, a)?);
                }
                // #5703: the leading real params BEFORE the synth `arguments`
                // slot (`static method(x, _ = 0) { … arguments … }` →
                // params `[x, _, <arguments>]`) must receive their positional
                // values, padded with `undefined` when under-supplied — exactly
                // as the class-DECLARATION (StaticMethodCall) path does.
                // Previously this branch pushed ONLY the arguments object, so a
                // leading param like `x` received the (empty) arguments array
                // instead of its argument / `undefined` (test262
                // `params-dflt-meth-static-args-unmapped`).
                let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                let fixed_count = declared.saturating_sub(1);
                for i in 0..fixed_count {
                    lowered.push(vals.get(i).cloned().unwrap_or_else(|| undef.clone()));
                }
                // The synthesized `arguments` object holds ALL passed args.
                let cap = (vals.len() as u32).to_string();
                let mut current = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
                for v in &vals {
                    let blk = ctx.block();
                    current = blk.call(I64, "js_array_push_f64", &[(I64, &current), (DOUBLE, v)]);
                }
                current =
                    ctx.block()
                        .call(I64, "js_array_mark_arguments_object", &[(I64, &current)]);
                let arguments_box = nanbox_pointer_inline(ctx.block(), &current);
                lowered.push(arguments_box);
            } else if has_rest {
                let fixed_count = declared.saturating_sub(1);
                for a in args.iter().take(fixed_count) {
                    lowered.push(lower_expr(ctx, a)?);
                }
                // #5703 (mirrors #235 in the StaticMethodCall path): when the
                // caller under-supplies the fixed leading params, pad the
                // missing slots with `undefined` BEFORE the rest array, so the
                // callee's default-param prologue / destructuring fires instead
                // of reading an uninitialized (0.0) parameter register.
                let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                while lowered.len() < fixed_count {
                    lowered.push(undef.clone());
                }
                let rest_count = args.len().saturating_sub(fixed_count);
                let cap = (rest_count as u32).to_string();
                let mut current = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
                for a in args.iter().skip(fixed_count) {
                    let v = lower_expr(ctx, a)?;
                    let blk = ctx.block();
                    current = blk.call(I64, "js_array_push_f64", &[(I64, &current), (DOUBLE, &v)]);
                }
                let rest_box = nanbox_pointer_inline(ctx.block(), &current);
                lowered.push(rest_box);
            } else {
                for a in args {
                    lowered.push(lower_expr(ctx, a)?);
                }
                // #5703: a static method of a class EXPRESSION called with fewer
                // args than declared (`C.m()` for `static m(a = 1)` or
                // `static m([x, y] = […])`) reaches this fused get-static-method
                // +call path rather than the `StaticMethodCall` path used by
                // class DECLARATIONS. That path pads missing slots with
                // `undefined` (#235); this one did not, so the callee read an
                // uninitialized (0.0) register — its default-param prologue
                // (`if (p === undefined) p = …`) and array destructuring
                // (`GetIterator(p)` → "is not iterable") never fired. Pad here
                // too so both paths behave identically.
                let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                while lowered.len() < declared {
                    lowered.push(undef.clone());
                }
            }
            let prev_this =
                ctx.block()
                    .call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, &recv_box)]);
            // Receiver-sensitive static `this` for plain class-ref receivers:
            // `D.f()` resolving to a parent's body at compile time must run
            // with `this === D` (the prologue's `js_static_this_resolve`
            // consumes this one-shot arm). Dynamic-value receiver shapes
            // (ClassExprFresh / factory Call / LocalGet) keep their prior
            // implicit-this-only behavior to avoid disturbing effect's
            // per-evaluation class-object statics.
            let plain_class_receiver =
                matches!(object, Expr::ClassRef(_) | Expr::ExternFuncRef { .. });
            if plain_class_receiver {
                ctx.block()
                    .call_void("js_static_this_arm_value", &[(DOUBLE, &recv_box)]);
            }
            let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
            let result = ctx.block().call(DOUBLE, &fn_name, &arg_slices);
            ctx.block()
                .call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, &prev_this)]);
            return Ok(Some(result));
        }
        // #1787 / #321: the call target is a static FIELD holding a callable,
        // not a static METHOD — e.g. effect's
        // `static make = (types) => ...` / `static unify = ...` on
        // `SchemaAST.Union`. The static-method walk above misses it (it's a
        // field), and the `js_class_static_method_call` fallback below returns
        // the receiver class ref on a method miss (an INT32 class id, which is
        // why `Union.make([...])` came back as `1`/undefined and Schema decode
        // died reading `_tag`). Detect a string-named static field on the
        // class's chain, read its value (the installed closure) via
        // `StaticFieldGet`, and invoke it with the call args. Static-field
        // arrows don't use dynamic `this`, so a plain closure call is correct.
        {
            let mut field_owner: Option<String> = None;
            let mut fc = Some(cls_name.clone());
            while let Some(c) = fc {
                if let Some(ci) = ctx.classes.get(&c) {
                    if ci
                        .static_fields
                        .iter()
                        .any(|f| f.key_expr.is_none() && f.name == *property)
                    {
                        field_owner = Some(c.clone());
                        break;
                    }
                }
                fc = ctx.classes.get(&c).and_then(|cc| cc.extends_name.clone());
            }
            if let Some(owner) = field_owner {
                let callee_val = lower_expr(
                    ctx,
                    &Expr::StaticFieldGet {
                        class_name: owner,
                        field_name: property.to_string(),
                    },
                )?;
                let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
                for a in args {
                    lowered_args.push(lower_expr(ctx, a)?);
                }
                let (args_ptr_i64, args_len) = if lowered_args.is_empty() {
                    ("0".to_string(), "0".to_string())
                } else {
                    let n = lowered_args.len();
                    let buf_reg = ctx.func.alloca_entry_array(DOUBLE, n);
                    for (i, v) in lowered_args.iter().enumerate() {
                        let slot = ctx
                            .block()
                            .gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                        ctx.block().store(DOUBLE, v, &slot);
                    }
                    let ptr_reg = ctx.block().next_reg();
                    ctx.block().emit_raw(format!(
                        "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                        ptr_reg, n, buf_reg
                    ));
                    let ptr_i64 = ctx.block().ptrtoint(&ptr_reg, I64);
                    (ptr_i64, n.to_string())
                };
                return Ok(Some(ctx.block().call(
                    DOUBLE,
                    "js_native_call_value",
                    &[
                        (DOUBLE, &callee_val),
                        (I64, &args_ptr_i64),
                        (I64, &args_len),
                    ],
                )));
            }
        }
        // No static method resolved through the class's statically-visible
        // chain. #1788: a subclass of a class-expression value
        // (`class Sub extends make(...) {}`) inherits the parent's static
        // methods at RUNTIME — dispatch through the class_id parent-chain
        // walk in CLASS_STATIC_METHODS, binding `this` to the class ref so
        // `this.<field>` resolves through the subclass's static-field chain.
        // The helper returns the receiver unchanged on a genuine miss, which
        // preserves the prior "yield the class ref for a chained `.pipe()`
        // during module init" behavior for truly-absent methods.
        //
        // #1787 / #321: also route imported-class receivers
        // (`ExternFuncRef("C")` from `import { C }`, or a `namespace.Class`
        // PropertyGet — effect's `AST.Union.make`). Their class stub has empty
        // compile-time static methods/fields, so resolution above misses; the
        // runtime call resolves both static methods AND static fields from the
        // class_id registries. `resolve_static_dispatch_cls` already gated
        // these on known-class membership, so reaching here means the receiver
        // really is a class.
        let receiver_is_dispatchable_class = matches!(object, Expr::ClassRef(_))
            || matches!(object, Expr::ExternFuncRef { name, .. } if ctx.class_ids.contains_key(name))
            || matches!(object, Expr::PropertyGet { object: inner, property }
                if matches!(inner.as_ref(), Expr::ExternFuncRef { .. }) && ctx.class_ids.contains_key(property));
        if receiver_is_dispatchable_class {
            let recv_box = lower_expr(ctx, object)?;
            let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
            for a in args {
                lowered_args.push(lower_expr(ctx, a)?);
            }
            // Materialize the args into an entry-block `[N x double]` slot
            // (see issue #167 — alloca must live in the entry block).
            let (args_ptr, args_len) = if lowered_args.is_empty() {
                ("null".to_string(), "0".to_string())
            } else {
                let n = lowered_args.len();
                let buf_reg = ctx.func.alloca_entry_array(DOUBLE, n);
                for (i, v) in lowered_args.iter().enumerate() {
                    let slot = ctx
                        .block()
                        .gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                    ctx.block().store(DOUBLE, v, &slot);
                }
                let ptr_reg = ctx.block().next_reg();
                ctx.block().emit_raw(format!(
                    "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                    ptr_reg, n, buf_reg
                ));
                (ptr_reg, n.to_string())
            };
            let key_idx = ctx.strings.intern(property);
            let entry = ctx.strings.entry(key_idx);
            let bytes_global = format!("@{}", entry.bytes_global);
            let name_len = entry.byte_len.to_string();
            let blk = ctx.block();
            let name_ptr_i64 = blk.ptrtoint(&bytes_global, I64);
            return Ok(Some(blk.call(
                DOUBLE,
                "js_class_static_method_call",
                &[
                    (DOUBLE, &recv_box),
                    (I64, &name_ptr_i64),
                    (I64, &name_len),
                    (crate::types::PTR, &args_ptr),
                    (I64, &args_len),
                ],
            )));
        }
        // For LocalGet receivers that resolve to a class but the
        // method isn't a static — fall through to the normal
        // instance/dynamic dispatch tower below.
    }
    Ok(None)
}
