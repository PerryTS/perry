//! New / NewDynamic.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::{anyhow, bail, Result};
#[allow(unused_imports)]
use perry_hir::{BinaryOp, CompareOp, Expr, UnaryOp, UpdateOp};
#[allow(unused_imports)]
use perry_types::Type as HirType;

#[allow(unused_imports)]
use crate::lower_call::{lower_call, lower_native_method_call, lower_new};
#[allow(unused_imports)]
use crate::lower_conditional::{lower_conditional, lower_logical, lower_truthy};
#[allow(unused_imports)]
use crate::lower_string_method::{
    flatten_string_add_chain, lower_string_coerce_concat, lower_string_concat,
    lower_string_concat_chain, lower_string_self_append,
};
#[allow(unused_imports)]
use crate::nanbox::{double_literal, POINTER_MASK_I64};
#[allow(unused_imports)]
use crate::type_analysis::{
    compute_auto_captures, is_array_expr, is_bigint_expr, is_bool_expr, is_map_expr,
    is_numeric_expr, is_set_expr, is_string_expr, is_url_search_params_expr, receiver_class_name,
};
#[allow(unused_imports)]
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

#[allow(unused_imports)]
use super::{
    buffer_alias_metadata_suffix, can_lower_expr_as_i32, emit_layout_note_slot_on_block,
    emit_shadow_slot_clear, emit_shadow_slot_update_for_expr, emit_string_literal_global,
    emit_v8_export_call, emit_v8_member_method_call, emit_write_barrier,
    emit_write_barrier_slot_on_block, expr_is_known_non_pointer_shadow_value,
    extract_array_of_object_shape, i32_bool_to_nanbox, import_origin_suffix,
    is_global_this_builtin_function_name, is_global_this_builtin_name, is_known_finite,
    lower_array_literal, lower_channel_reduction, lower_expr, lower_expr_as_i32,
    lower_index_set_fast, lower_js_args_array, lower_object_literal, lower_stream_super_init,
    lower_url_string_getter, nanbox_bigint_inline, nanbox_pointer_inline,
    nanbox_pointer_inline_pub, nanbox_string_inline, proxy_build_args_array, try_flat_const_2d_int,
    try_lower_flat_const_index_get, try_match_channel_reduction, try_static_class_name,
    unbox_str_handle, unbox_to_i64, variant_name, ChannelReduction, FlatConstInfo, FnCtx,
    I18nLowerCtx,
};

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::New {
            class_name, args, ..
        } => lower_new(ctx, class_name, args),

        // `new <expr>(args…)` where the callee isn't a bare identifier.
        // Several shapes get static rerouting; the rest fall back to a
        // best-effort empty-object placeholder so the binary still
        // compiles.
        //
        // Cases handled (in priority order):
        //
        //   1. `new ClassRef("Foo")` — the HIR's `Expr::ClassRef` is what
        //      a class identifier referenced as a value lowers to (see
        //      `crates/perry-hir/src/lower.rs::ast::Expr::Ident` →
        //      `Expr::ClassRef` at line ~4480). When the parser sees
        //      `new (Foo)()` or `new (someParen)()` where the inner is a
        //      class name, the callee comes through as `ClassRef("Foo")`.
        //      Reroute straight to `lower_new`.
        //
        //   2. `new globalThis.WebSocket(url)` — the parser emits this as
        //      `NewDynamic { callee: PropertyGet { GlobalGet(_), "WebSocket" }, args }`
        //      (used for built-ins like WebSocket / Date / Map / etc. that
        //      live on the global object). Reroute to `lower_new(name)`
        //      so the existing built-in/runtime class handling kicks in.
        //
        //   3. `new (condition ? A : B)()` — emit a runtime conditional
        //      where each arm runs `lower_new` (or recursively the
        //      NewDynamic fallback) on its own branch. We synthesize
        //      `NewDynamic { callee: A, args }` and `NewDynamic { callee: B, args }`,
        //      then call `lower_conditional` to emit the standard
        //      cond_br/phi pattern. Args are cloned for each branch — fine
        //      because `new` args are typically simple expressions, and
        //      side effects fire under the conditional's cond_br anyway
        //      (matching JS evaluation semantics where the unchosen arm
        //      doesn't run).
        //
        //   4. Anything else (`new someVar()`, `new this.something()`,
        //      `new someFn()()`) — lower the callee + args for side
        //      effects (closures, string literal interning, lazy declares)
        //      and return an empty-object placeholder. The runtime won't
        //      dispatch correctly here — calling a method on the result
        //      will return `undefined` — but the binary compiles instead
        //      of failing the whole module. Real fix requires a runtime
        //      `js_new_dynamic(callee_value, args_vec)` helper that
        //      inspects the callee's NaN tag and dispatches to the right
        //      class constructor. That's a separate followup tracked in
        //      the v0.5.8 changelog.
        Expr::NewDynamic { callee, args } => {
            // Case 1 + 2: callee is statically a class.
            if let Some(name) = try_static_class_name(callee.as_ref(), ctx) {
                return lower_new(ctx, name, args);
            }

            // date-fns `constructFrom(date, value)`:
            //   return new date.constructor(value);
            // The callee is `PropertyGet { LocalGet(date), "constructor" }`
            // where `date` is statically Date-typed. Lower through the
            // dedicated `Expr::DateNew` path so the call routes to
            // `js_date_new_from_value` / `js_date_new_local_components`
            // and the result is a real Date timestamp, not an empty
            // ObjectHeader. Pre-fix the NewDynamic fallback returned a
            // placeholder object — `cloned.getTime()` then read garbage
            // and the equality failed. Refs date-fns blocker.
            if let Expr::PropertyGet { object, property } = callee.as_ref() {
                if property == "constructor" {
                    if let Expr::LocalGet(id) = object.as_ref() {
                        let is_date = matches!(
                            ctx.local_types.get(id),
                            Some(HirType::Named(name)) if name == "Date"
                        );
                        if is_date {
                            let synth = Expr::DateNew(args.to_vec());
                            return lower_expr(ctx, &synth);
                        }
                    }
                }
            }

            // `new assert.AssertionError(options)` — Node's `assert`
            // exposes a real constructor that accepts a `{actual,
            // expected, operator, message, generatedMessage}` options
            // bag and produces an AssertionError instance that
            // satisfies `instanceof Error`. Route to the runtime
            // helper directly so user code doesn't have to call
            // through a synthesized closure (the helper lives in
            // perry-runtime/src/object/mod.rs and reuses the same
            // make_assertion_error path the failing-assert helpers
            // already use, so the resulting instance has the same
            // class_id-extends-Error registration).
            if let Expr::PropertyGet { object, property } = callee.as_ref() {
                if property == "AssertionError" {
                    if let Expr::NativeModuleRef(mod_name) = object.as_ref() {
                        if mod_name == "assert" || mod_name == "assert/strict" {
                            let opts = if args.is_empty() {
                                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                            } else {
                                lower_expr(ctx, &args[0])?
                            };
                            return Ok(ctx.block().call(
                                DOUBLE,
                                "js_assert_assertion_error_ctor",
                                &[(DOUBLE, &opts)],
                            ));
                        }
                    }
                }
                if property == "Assert" {
                    if let Expr::NativeModuleRef(mod_name) = object.as_ref() {
                        if mod_name == "assert" || mod_name == "assert/strict" {
                            let opts = if args.is_empty() {
                                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                            } else {
                                lower_expr(ctx, &args[0])?
                            };
                            return Ok(ctx.block().call(
                                DOUBLE,
                                "js_assert_assert_ctor",
                                &[(DOUBLE, &opts)],
                            ));
                        }
                    }
                }
            }

            // `new net.BlockList()` / `new net.SocketAddress(options)` are
            // native-module constructor exports, so their callee arrives as
            // `PropertyGet { NativeModuleRef("net"), ... }` rather than a bare
            // built-in class name. Route them through `lower_new` so the
            // handle-producing constructor arms allocate registered net handles.
            if let Expr::PropertyGet { object, property } = callee.as_ref() {
                if matches!(property.as_str(), "BlockList" | "SocketAddress") {
                    if let Expr::NativeModuleRef(mod_name) = object.as_ref() {
                        if mod_name == "net" || mod_name == "node:net" {
                            return lower_new(ctx, property, args);
                        }
                    }
                }
            }

            // `new crypto.Certificate()` is a legacy constructor in Node, but
            // the implementation is a stateless namespace over the same SPKAC
            // helper methods as `crypto.Certificate.*`. Represent instances as
            // the `crypto.Certificate` native namespace so method calls dispatch
            // through the existing native-module path.
            if let Expr::PropertyGet { object, property } = callee.as_ref() {
                if property == "Certificate" && args.is_empty() {
                    if let Expr::NativeModuleRef(mod_name) = object.as_ref() {
                        if mod_name == "crypto" {
                            let module_name = "crypto.Certificate";
                            let mod_idx = ctx.strings.intern(module_name);
                            let mod_bytes_global =
                                format!("@{}", ctx.strings.entry(mod_idx).bytes_global);
                            let mod_len_str = module_name.len().to_string();
                            return Ok(ctx.block().call(
                                DOUBLE,
                                "js_create_native_module_namespace",
                                &[(PTR, &mod_bytes_global), (I64, &mod_len_str)],
                            ));
                        }
                    }
                }
            }

            // `new v8.GCProfiler()` (#3142) — represent the profiler instance
            // as the `"v8.GCProfiler"` native-module namespace so its
            // `start()` / `stop()` methods dispatch through the runtime
            // native-module method table (same shape as `new crypto.Certificate`).
            if let Expr::PropertyGet { object, property } = callee.as_ref() {
                if property == "GCProfiler" {
                    if let Expr::NativeModuleRef(mod_name) = object.as_ref() {
                        if mod_name == "v8" {
                            for a in args {
                                let _ = lower_expr(ctx, a)?;
                            }
                            let module_name = "v8.GCProfiler";
                            let mod_idx = ctx.strings.intern(module_name);
                            let mod_bytes_global =
                                format!("@{}", ctx.strings.entry(mod_idx).bytes_global);
                            let mod_len_str = module_name.len().to_string();
                            return Ok(ctx.block().call(
                                DOUBLE,
                                "js_create_native_module_namespace",
                                &[(PTR, &mod_bytes_global), (I64, &mod_len_str)],
                            ));
                        }
                    }
                }
            }

            // `new stream.Readable(opts)` / `new stream.Writable(opts)` /
            // `new stream.Duplex(...)` / `.Transform` / `.PassThrough` (#3663).
            // The namespace-member form (`import * as stream` /
            // `const stream = require('stream')`) arrives here as
            // `NewDynamic { callee: PropertyGet { NativeModuleRef("stream"),
            // "Readable" } }` instead of the bare-identifier `Expr::New`
            // produced by a named ESM import. Without this arm it would fall
            // through to the empty-object placeholder below, so the resulting
            // object carries no EventEmitter/Writable methods and
            // `.on()`/`.write()`/`.pipe()` throw "is not a function". Route to
            // the same `lower_builtin_new` stream handler the named-import path
            // uses so the runtime allocates the fully-methoded stream object.
            if let Expr::PropertyGet { object, property } = callee.as_ref() {
                if let Expr::NativeModuleRef(mod_name) = object.as_ref() {
                    if mod_name == "stream"
                        && matches!(
                            property.as_str(),
                            "Readable" | "Writable" | "Duplex" | "Transform" | "PassThrough"
                        )
                    {
                        return lower_new(ctx, property, args);
                    }
                }
            }

            // `new v8.Serializer()` / `new v8.Deserializer(buf)` (and the
            // `Default*` subclasses) (#3680) — route to the runtime
            // constructors that allocate a codec-backed instance object whose
            // methods dispatch through the native-module method table.
            if let Expr::PropertyGet { object, property } = callee.as_ref() {
                if let Expr::NativeModuleRef(mod_name) = object.as_ref() {
                    if mod_name == "v8" {
                        match property.as_str() {
                            "Serializer" | "DefaultSerializer" => {
                                for a in args {
                                    let _ = lower_expr(ctx, a)?;
                                }
                                let is_default = property == "DefaultSerializer";
                                let flag =
                                    crate::nanbox::double_literal(f64::from_bits(if is_default {
                                        0x7FFC_0000_0000_0004 // TAG_TRUE
                                    } else {
                                        crate::nanbox::TAG_UNDEFINED
                                    }));
                                return Ok(ctx.block().call(
                                    DOUBLE,
                                    "js_v8_serializer_new",
                                    &[(DOUBLE, &flag)],
                                ));
                            }
                            "Deserializer" | "DefaultDeserializer" => {
                                let buf = if let Some(first) = args.first() {
                                    lower_expr(ctx, first)?
                                } else {
                                    crate::nanbox::double_literal(f64::from_bits(
                                        crate::nanbox::TAG_UNDEFINED,
                                    ))
                                };
                                for extra in args.iter().skip(1) {
                                    let _ = lower_expr(ctx, extra)?;
                                }
                                return Ok(ctx.block().call(
                                    DOUBLE,
                                    "js_v8_deserializer_new",
                                    &[(DOUBLE, &buf)],
                                ));
                            }
                            _ => {}
                        }
                    }
                }
            }

            // `new (PerformanceObserver as any)(cb?)` — the `as any` cast
            // (used because no-arg construction is a TS type error) strips the
            // bare identifier, so the constructor arrives as
            // `NewDynamic { callee: PropertyGet { NativeModuleRef("perf_hooks"),
            // "PerformanceObserver" } }` instead of the special-cased `New`
            // handled in lower_call/builtin.rs. Route to the same runtime
            // registrar so the no-callback TypeError (and normal construction)
            // fire. Refs #1388.
            if let Expr::PropertyGet { object, property } = callee.as_ref() {
                if property == "PerformanceObserver" {
                    if let Expr::NativeModuleRef(mod_name) = object.as_ref() {
                        if mod_name == "perf_hooks" {
                            let cb = if args.is_empty() {
                                crate::nanbox::double_literal(f64::from_bits(
                                    crate::nanbox::TAG_UNDEFINED,
                                ))
                            } else {
                                lower_expr(ctx, &args[0])?
                            };
                            return Ok(ctx.block().call(
                                DOUBLE,
                                "js_perf_observer_new",
                                &[(DOUBLE, &cb)],
                            ));
                        }
                    }
                }
            }

            // Refs #740: `new O.Inner(args)` where `O` is an object
            // literal whose `Inner` field was initialized from a class
            // expression. The Stmt::Let lowering populates
            // `local_class_field_aliases[O_id]["Inner"] = "__anon_class_N"`
            // when it sees the original literal — read it back here and
            // dispatch to `lower_new` instead of the empty-object
            // fallback.
            if let Expr::PropertyGet { object, property } = callee.as_ref() {
                if let Expr::LocalGet(obj_id) = object.as_ref() {
                    if let Some(class_name) = ctx
                        .local_class_field_aliases
                        .get(obj_id)
                        .and_then(|f| f.get(property))
                        .cloned()
                    {
                        return lower_new(ctx, &class_name, args);
                    }
                }
            }

            // Case 3: callee is a ternary. Synthesize a NewDynamic for
            // each branch and emit a runtime if/else with phi. The inner
            // NewDynamics fall through this same handler — if they're
            // statically resolvable they reroute to lower_new; otherwise
            // they fall back to the empty-object placeholder. Either way
            // each branch produces a valid double for the phi to merge.
            if let Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } = callee.as_ref()
            {
                let then_synth = Expr::NewDynamic {
                    callee: then_expr.clone(),
                    args: args.clone(),
                };
                let else_synth = Expr::NewDynamic {
                    callee: else_expr.clone(),
                    args: args.clone(),
                };
                return lower_conditional(ctx, condition, &then_synth, &else_synth);
            }

            // Issue #838 followup (b): callee is a function declaration
            // (or any expression that evaluates to a callable closure).
            // Route through the runtime construct helper so the
            // synthetic class id allocated against the closure's bits
            // (in `js_register_function_prototype_method`) lands on the
            // instance header — dispatch then finds the
            // prototype-registered methods via the regular
            // `(*obj).class_id → CLASS_PROTOTYPE_METHODS` walk. The
            // helper also binds `this` to the new instance for the
            // duration of the constructor call so `this.<field> = …`
            // writes in the function body land on the instance.
            //
            // We use this path for `FuncRef`-callee NewDynamics. Other
            // dynamic shapes (`new someVar()`, `new someExpr()`) still
            // fall through to the empty-object placeholder — extending
            // there is a separate followup (`js_new_dynamic` proper).
            // Issue #838 followup (b): also route LocalGet callees
            // through the construct helper. dayjs's outer-scope shape
            // assigns the IIFE result to a local (`var Klass = (function
            // (){ function M(){…}; M.prototype.x = fn; return M; })()`),
            // so `new Klass(args)` reaches here as
            // `NewDynamic { callee: LocalGet(Klass_id), … }` — the
            // helper looks up the synthetic class id by NaN-boxed bits,
            // matching the registration site that used the same local.
            // Generic NewDynamic callees with unknown closure shape are
            // also supported because the helper falls back to a
            // class_id=0 empty-object allocation when no synthetic id
            // exists (preserves the pre-fix baseline).
            // Also route PropertyGet callees through `js_new_function_construct`:
            // covers `new date.constructor(value)` (date-fns
            // `constructFrom`) and generic `new obj.factory(...)` shapes
            // where `obj.factory` resolves to a closure pointer at
            // runtime. The runtime helper detects the global Date /
            // Array / Object thunks and dispatches into the matching
            // real factory; non-matching closures still get the
            // class_id=0 empty-object baseline.
            let routes_through_function_construct = matches!(
                callee.as_ref(),
                Expr::FuncRef(_) | Expr::LocalGet(_) | Expr::PropertyGet { .. }
            );
            if routes_through_function_construct {
                let func_double = lower_expr(ctx, callee)?;
                let lowered_args: Vec<String> = args
                    .iter()
                    .map(|a| lower_expr(ctx, a))
                    .collect::<Result<Vec<_>>>()?;
                let (args_ptr, args_len) = lower_js_args_array(ctx, &lowered_args);
                let result = ctx.block().call(
                    DOUBLE,
                    "js_new_function_construct",
                    &[(DOUBLE, &func_double), (PTR, &args_ptr), (I64, &args_len)],
                );
                return Ok(result);
            }

            // Case 4: best-effort fallback. Lower the callee + args for
            // side effects, then return an empty object as the result.
            let _ = lower_expr(ctx, callee)?;
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let class_id = "0".to_string();
            let count = "0".to_string();
            let handle =
                ctx.block()
                    .call(I64, "js_object_alloc", &[(I32, &class_id), (I32, &count)]);
            Ok(nanbox_pointer_inline(ctx.block(), &handle))
        }

        // `this` — load from the topmost `this` slot in the constructor
        // stack. When `this_stack` is empty (top-level module code, top-
        // level function declarations, or non-arrow function expressions
        // without `captures_this`), fall through to the runtime
        // IMPLICIT_THIS thread-local. Issue #519: `js_native_call_method`'s
        // field-scan dispatch path saves/sets IMPLICIT_THIS to the
        // receiver before calling a closure-typed class field, so the
        // function body's `this` correctly references the calling
        // instance. When the function is invoked outside a method-style
        // call, IMPLICIT_THIS stays at its initial TAG_UNDEFINED — same
        // observable behavior as the previous 0.0 sentinel for the
        // strict-mode top-level case.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
