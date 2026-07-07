//! This / SuperCall.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
#[allow(unused_imports)]
use perry_hir::{BinaryOp, CompareOp, Expr, UnaryOp, UpdateOp};
#[allow(unused_imports)]
use perry_types::Type as HirType;

#[allow(unused_imports)]
use crate::lower_call::{
    bind_inline_constructor_params, lower_call, lower_native_method_call, lower_new,
    restore_inline_constructor_scope,
};
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
    lower_array_literal, lower_array_super_init, lower_channel_reduction,
    lower_event_emitter_subclass_init, lower_expr, lower_expr_as_i32, lower_index_set_fast,
    lower_js_args_array, lower_node_stream_super_init, lower_object_literal,
    lower_stream_super_init, lower_url_string_getter, nanbox_bigint_inline, nanbox_pointer_inline,
    nanbox_pointer_inline_pub, nanbox_string_inline, proxy_build_args_array, try_flat_const_2d_int,
    try_lower_flat_const_index_get, try_match_channel_reduction, try_static_class_name,
    unbox_str_handle, unbox_to_i64, variant_name, ChannelReduction, FlatConstInfo, FnCtx,
    I18nLowerCtx,
};

/// Built-in constructor names (beyond Error/stream/fetch, which have their own
/// SuperCall arms) that can appear as a class heritage. `super(...)` to these
/// must NOT be routed through the runtime-value dispatch path
/// (`js_fetch_or_value_super`), which would invoke e.g. `Map()` without `new`
/// and throw "Constructor requires 'new'". Perry cannot yet give a subclass
/// instance the built-in's internal slots, so `super()` is a best-effort no-op
/// here — enough that `class M extends Map { constructor(){ super(); } }`
/// constructs without throwing. Refs class/subclass/builtin-objects/*/
/// super-must-be-called.
pub(crate) fn is_other_builtin_constructor_name(name: &str) -> bool {
    matches!(
        name,
        "Map"
            | "Set"
            | "WeakMap"
            | "WeakSet"
            | "Array"
            | "ArrayBuffer"
            | "SharedArrayBuffer"
            | "DataView"
            | "Boolean"
            | "Number"
            | "String"
            | "Date"
            | "RegExp"
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

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::This => {
            if let Some(slot) = ctx.this_stack.last().cloned() {
                Ok(ctx.block().load(DOUBLE, &slot))
            } else {
                let helper = if ctx.is_strict_fn {
                    "js_implicit_this_get"
                } else {
                    "js_implicit_this_get_sloppy"
                };
                Ok(ctx.block().call(DOUBLE, helper, &[]))
            }
        }
        Expr::NewTarget => {
            if let Some(slot) = ctx.new_target_stack.last().cloned() {
                Ok(ctx.block().load(DOUBLE, &slot))
            } else {
                Ok(ctx.block().call(DOUBLE, "js_new_target_get", &[]))
            }
        }

        // `super(args…)` — Phase C.2 inheritance. Look up the current
        // class's parent and inline the parent's constructor body
        // with the SAME `this` (so parent fields end up on the same
        // object). Parent's parameters get fresh slots populated with
        // the lowered super-call args.
        //
        // The current class is the topmost entry in `class_stack`. The
        // `super(...spread)` — tsc's pass-through ctor (`constructor(){
        // super(...arguments) }`, zod's ZodNumber/ZodBigInt). The arg
        // count is dynamic, so the parent ctor can't be inlined; build
        // the args array and invoke the closest registered ancestor ctor
        // on the SAME `this` through the CLASS_CONSTRUCTORS registry.
        Expr::SuperCallSpread(call_args) => {
            let Some(current_class_name) = ctx.class_stack.last().cloned() else {
                for a in call_args {
                    let (perry_hir::CallArg::Expr(e) | perry_hir::CallArg::Spread(e)) = a;
                    let _ = lower_expr(ctx, e)?;
                }
                return Ok(double_literal(0.0));
            };
            // Materialize the args array (spread elements appended via
            // the runtime spread helper).
            let zero = "0".to_string();
            let mut arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &zero)]);
            for a in call_args {
                match a {
                    perry_hir::CallArg::Expr(e) => {
                        let v = lower_expr(ctx, e)?;
                        arr = ctx.block().call(
                            I64,
                            "js_array_push_f64",
                            &[(I64, &arr), (DOUBLE, &v)],
                        );
                    }
                    perry_hir::CallArg::Spread(e) => {
                        // Route every spread operand through the full iterator
                        // protocol (`js_array_spread_append` -> `array_from_
                        // spread_value`): it drives a custom `[Symbol.iterator]`
                        // (`super(...iter)`), spreads the arguments OBJECT
                        // (`super(...arguments)`), arrays, sets/maps, typed
                        // arrays, and strings, AND propagates an abrupt
                        // completion from a throwing iterator step/value — the
                        // `call-spread-*-iter` / `call-spread-err-*` cases. The
                        // old `js_array_push_spread_any` only handled arrays and
                        // array-like (`.length`) objects, so a plain iterable
                        // (no `.length`) contributed zero args.
                        let v = lower_expr(ctx, e)?;
                        arr = ctx.block().call(
                            I64,
                            "js_array_spread_append",
                            &[(I64, &arr), (DOUBLE, &v)],
                        );
                    }
                }
            }
            // Invoke the closest registered ancestor ctor through the
            // CLASS_CONSTRUCTORS registry. KNOWN GAP: constructions from
            // METHOD bodies (standalone-ctor path) currently lose the
            // parent's field writes — see the wall-21 notes; top-level and
            // arrow-context constructions work.
            let this_box = match ctx.this_stack.last().cloned() {
                Some(slot) => ctx.block().load(DOUBLE, &slot),
                None => double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
            };
            // `class X extends Map | Set` with a spread super (`super(...args)`,
            // e.g. NestJS's `ModulesContainer`'s `super(...arguments)`) — install
            // the hidden collection backing from the (possibly spread) args
            // array instead of dispatching the uncallable builtin ctor. The
            // first array element (if any) is the iterable; `js_map_from_iterable`
            // / `js_set_from_iterable` ignore extra elements. Mirrors the
            // non-spread `Expr::SuperCall` Map/Set arm.
            let map_set_kind = ctx
                .classes
                .get(&current_class_name)
                .and_then(|c| c.extends_name.as_deref())
                .and_then(|p| match p {
                    "Map" => Some(0i32),
                    "Set" => Some(1i32),
                    _ => None,
                });
            if let Some(kind) = map_set_kind {
                let blk = ctx.block();
                let arr_box = nanbox_pointer_inline(blk, &arr);
                let zero_idx = "0".to_string();
                let first =
                    ctx.block()
                        .call(DOUBLE, "js_array_get_f64", &[(I64, &arr), (I32, &zero_idx)]);
                let _ = arr_box;
                ctx.block().call(
                    DOUBLE,
                    "js_map_set_subclass_init",
                    &[
                        (DOUBLE, &this_box),
                        (I32, &kind.to_string()),
                        (DOUBLE, &first),
                    ],
                );
                crate::lower_call::apply_field_initializers_recursive(
                    ctx,
                    &current_class_name,
                    crate::lower_call::FieldInitMode::SelfOnly,
                )?;
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            if let Some(&child_cid) = ctx.class_ids.get(&current_class_name) {
                let cid_str = child_cid.to_string();
                let blk = ctx.block();
                let arr_box = nanbox_pointer_inline(blk, &arr);
                ctx.block().call_void(
                    "js_super_construct_apply",
                    &[(I32, &cid_str), (DOUBLE, &this_box), (DOUBLE, &arr_box)],
                );
            }
            // Spec: subclass field initializers run AFTER super() returns
            // (mirrors every other super arm).
            crate::lower_call::apply_field_initializers_recursive(
                ctx,
                &current_class_name,
                crate::lower_call::FieldInitMode::SelfOnly,
            )?;
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }
        // parent is `current_class.extends_name` (Perry uses the string
        // form for cross-module/late-resolved cases) or
        // `current_class.extends.and_then(class_id_to_name)`. For Phase
        // C.2 we use `extends_name` which is always populated when
        // there's a parent.
        Expr::SuperCall(super_args) => {
            // Soft fallback for super() outside a class context: lower
            // args and return undefined.
            let Some(current_class_name) = ctx.class_stack.last().cloned() else {
                for a in super_args {
                    let _ = lower_expr(ctx, a)?;
                }
                return Ok(double_literal(0.0));
            };
            let current_class = match ctx.classes.get(&current_class_name).copied() {
                Some(c) => c,
                None => {
                    for a in super_args {
                        let _ = lower_expr(ctx, a)?;
                    }
                    return Ok(double_literal(0.0));
                }
            };
            let parent_name = match current_class.extends_name.as_deref() {
                Some(s) => s.to_string(),
                // A lexically-shadowed / fully-dynamic parent carries no
                // `extends_name` (the parent is a runtime value, not a named
                // class) but DOES carry `extends_expr`. Proceed with an empty
                // placeholder name — for this shape `static_parent_lookup` below
                // is forced to `None` (extends_expr present) and the builtin-name
                // gate is disabled by `heritage_lexically_shadowed`, so the name
                // is never consulted; `super()` dispatches via `extends_expr`.
                // Without this, `super()` in such a subclass silently no-ops and
                // the (dynamic) parent constructor never runs.
                None if current_class.extends_expr.is_some() => String::new(),
                None => {
                    for a in super_args {
                        let _ = lower_expr(ctx, a)?;
                    }
                    return Ok(double_literal(0.0));
                }
            };
            // #5437 (Next.js p-queue `PQueue`): when HIR captured a dynamic
            // `extends_expr` for this class, the parent is a LEXICAL runtime
            // value (an in-scope local / require result) — NOT the same-named
            // module-global class that `ctx.classes.get(parent_name)` would
            // wrongly return (minified turbopack chunks reuse single-letter
            // class names across webpack factories). Force the `None` arm's
            // dynamic-parent dispatch so `super()` invokes the real lexical
            // parent value, mirroring the synthesized-ctor dynamic-parent path
            // in `codegen/method.rs`. Without this, `PQueue extends t` resolved
            // `t` to superstruct's `StructError` base and `super()` inlined its
            // destructuring ctor on the undefined options arg → HTTP 500.
            let static_parent_lookup = if current_class.extends_expr.is_some() {
                None
            } else {
                ctx.classes.get(&parent_name).copied()
            };
            let parent_class = match static_parent_lookup {
                Some(c) => c,
                None => {
                    // #321 / #66 (#1787 follow-up): `class Sub extends <runtimeValueFn>`
                    // — the parent is a runtime-value function/closure (the IIFE-
                    // returned constructor function `Base` in Effect's `Data.Class`).
                    // HIR's `lower_decl/class_decl.rs` already captures
                    // `class.extends_expr` for this shape (unknown Ident super-class)
                    // and codegen wires the class_id parent edge at module init via
                    // `js_register_class_parent_dynamic`. The MISSING piece this arm
                    // adds is the `super(args)` call itself: evaluate the extends
                    // expression here, bind IMPLICIT_THIS to the current `this`, and
                    // dispatch via `js_native_call_value` so the parent function's
                    // body runs with `this` bound to the new instance (any
                    // `Object.assign(this, args)` / `this.x = args.x` writes land on
                    // the subclass instance). Falls through to the existing
                    // stream/Error-like/no-op chain when no extends_expr is captured
                    // (which is exactly the prior baseline).
                    //
                    // Gate: skip well-known built-in parent NAMES (Error/Stream
                    // family) — HIR captures `extends_expr` for any unknown Ident,
                    // INCLUDING the built-ins, so we'd otherwise eat the more-correct
                    // Error-init path below. The built-in arms handle their own
                    // semantics (Error sets this.message + this.name; streams allocate
                    // a registry handle). Anything else with an extends_expr is a
                    // real runtime-value parent and routes through this dispatch.
                    // The classic node:stream / Web-Streams names are only the
                    // genuine built-in parents when HIR did NOT capture an
                    // `extends_expr`. When it did, the parent is a userland
                    // stream-shim value (e.g. readable-stream's `Transform`,
                    // winston's `class Logger extends Transform`) whose real
                    // constructor — which sets `this._readableState`,
                    // `this._writableState`, `this._transformState` — must run.
                    // HIR's `is_genuine_node_stream_parent` gate only leaves
                    // `extends_expr` set for the non-builtin case (the genuine
                    // node:stream import keeps `native_extends` + no
                    // `extends_expr`), so deferring to the dynamic dispatch here
                    // whenever an `extends_expr` exists is safe.
                    let has_extends_expr = current_class.extends_expr.is_some();
                    let is_stream_family_name = matches!(
                        parent_name.as_str(),
                        "Readable"
                            | "Writable"
                            | "Duplex"
                            | "Transform"
                            | "ReadableStream"
                            | "WritableStream"
                            | "TransformStream"
                    );
                    let is_builtin_parent_name = (matches!(
                        parent_name.as_str(),
                        "Error"
                            | "TypeError"
                            | "RangeError"
                            | "ReferenceError"
                            | "SyntaxError"
                            | "URIError"
                            | "EvalError"
                            | "AggregateError"
                            | "Request"
                            | "Response"
                            | "Event"
                            | "CustomEvent"
                    ) || (is_stream_family_name
                        && !has_extends_expr)
                        || is_other_builtin_constructor_name(parent_name.as_str()))
                        && !(is_stream_family_name && has_extends_expr)
                        // #5437: a parent NAME shadowed by an in-scope lexical
                        // local is NOT the built-in — route it through the
                        // dynamic `extends_expr` value so `super()` runs the
                        // local's constructor (`const Error = class {…}; class X
                        // extends Error {}`), not the built-in Error initializer.
                        && !current_class.heritage_lexically_shadowed;
                    if !is_builtin_parent_name {
                        if let Some(extends_expr) = current_class.extends_expr.as_deref() {
                            // Lower the super-call args first so they get fresh slots
                            // and are spilled into a flat f64 buffer for the variadic
                            // dispatch.
                            let mut lowered_args: Vec<String> =
                                Vec::with_capacity(super_args.len());
                            for a in super_args {
                                lowered_args.push(lower_expr(ctx, a)?);
                            }

                            // Resolve the parent constructor VALUE. The decl-time
                            // `js_register_class_parent_dynamic` already evaluated
                            // `extends_expr` in the module-init scope (where its free
                            // variables — e.g. a require alias `_suffix` in
                            // `class X extends _suffix.default` — are bound) and
                            // stashed the result keyed by this class's id. Prefer the
                            // stashed value: re-evaluating `extends_expr` HERE runs in
                            // the constructor scope, where an IIFE-local require alias
                            // is NOT captured, so the member read would throw "Cannot
                            // read properties of undefined". Fall back to a fresh eval
                            // only when the class id is unknown at codegen time (the
                            // value was never stashed) or the stash is empty.
                            // The decl-time `RegisterClassParentDynamic` runs at
                            // module init, before any `new X()`, so a class that
                            // reaches this branch has reliably stashed its parent.
                            // Fall back to a fresh eval only when the class id is
                            // unknown at codegen time (no stash key).
                            let parent_val = match ctx.class_ids.get(&current_class_name).copied() {
                                Some(cid) if cid != 0 => ctx.block().call(
                                    DOUBLE,
                                    "js_get_dynamic_parent_value",
                                    &[(crate::types::I32, &cid.to_string())],
                                ),
                                _ => lower_expr(ctx, extends_expr)?,
                            };

                            // Spill args into a contiguous double[] for the
                            // js_native_call_value(ptr, len) ABI. Mirrors the
                            // method_override.rs override-path spilling.
                            let user_arg_count = lowered_args.len();
                            let (args_ptr, args_len) = if user_arg_count == 0 {
                                ("null".to_string(), "0".to_string())
                            } else {
                                let buf_reg = ctx.func.alloca_entry_array(DOUBLE, user_arg_count);
                                for (i, a_val) in lowered_args.iter().enumerate() {
                                    let slot = ctx.block().gep(
                                        DOUBLE,
                                        &buf_reg,
                                        &[(I64, &format!("{}", i))],
                                    );
                                    ctx.block().store(DOUBLE, a_val, &slot);
                                }
                                let ptr_reg = ctx.block().next_reg();
                                ctx.block().emit_raw(format!(
                                    "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                                    ptr_reg, user_arg_count, buf_reg
                                ));
                                (ptr_reg, user_arg_count.to_string())
                            };

                            // Bind IMPLICIT_THIS to the current `this` so the parent
                            // function body's `this.x = ...` writes land on the
                            // subclass instance (non-arrow functions read `this` via
                            // `js_implicit_this_get` when their this_stack is empty).
                            // Save the prior IMPLICIT_THIS and restore it after — see
                            // the #519 pattern in console_promise.rs / method_override.rs.
                            let this_box = match ctx.this_stack.last().cloned() {
                                Some(slot) => ctx.block().load(DOUBLE, &slot),
                                None => {
                                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                                }
                            };
                            // Route runtime-value super() through the
                            // fetch-aware dispatcher: when `parent_val` is the
                            // global Request/Response constructor (possibly via
                            // an alias like `@hono/node-server`'s
                            // `GlobalRequest = global.Request`), it allocates the
                            // native fetch handle and stashes it on `this` so
                            // inherited body methods resolve; otherwise it falls
                            // back to the ordinary implicit-`this`-bound
                            // `js_native_call_value` (unchanged behavior for
                            // every other runtime-value parent).
                            let _ = ctx.block().call(
                                DOUBLE,
                                "js_fetch_or_value_super",
                                &[
                                    (DOUBLE, &parent_val),
                                    (DOUBLE, &this_box),
                                    (crate::types::PTR, &args_ptr),
                                    (I64, &args_len),
                                ],
                            );

                            // Per JS spec: subclass field initializers run AFTER
                            // super() returns. Same call the user-class branch makes
                            // (line ~434 below) and the stream subclass branch makes
                            // above. Without this, `this.foo = []` on the subclass
                            // would never run.
                            crate::lower_call::apply_field_initializers_recursive(
                                ctx,
                                &current_class_name,
                                crate::lower_call::FieldInitMode::SelfOnly,
                            )?;

                            return Ok(double_literal(f64::from_bits(
                                crate::nanbox::TAG_UNDEFINED,
                            )));
                        }
                    }
                    let node_stream_kind = match parent_name.as_str() {
                        "Readable" => Some("readable"),
                        "Writable" => Some("writable"),
                        "Duplex" => Some("duplex"),
                        "Transform" => Some("transform"),
                        _ => None,
                    };
                    if let Some(kind) = node_stream_kind {
                        let result = lower_node_stream_super_init(ctx, kind, super_args)?;
                        let current_class_name =
                            ctx.class_stack.last().cloned().unwrap_or_default();
                        crate::lower_call::apply_field_initializers_recursive(
                            ctx,
                            &current_class_name,
                            crate::lower_call::FieldInitMode::SelfOnly,
                        )?;
                        return Ok(result);
                    }
                    // `class X extends Array` — size the subclass instance and
                    // install the Array surface (`fill`, …) it relies on. Perry
                    // models the instance as a plain object, not a real exotic
                    // Array (ArrayHeader has no class_id slot), so `super(n)`
                    // would otherwise leave it length-less with no Array methods.
                    if parent_name == "Array" {
                        let result = lower_array_super_init(ctx, super_args)?;
                        let current_class_name =
                            ctx.class_stack.last().cloned().unwrap_or_default();
                        crate::lower_call::apply_field_initializers_recursive(
                            ctx,
                            &current_class_name,
                            crate::lower_call::FieldInitMode::SelfOnly,
                        )?;
                        return Ok(result);
                    }
                    // Issue #562: `class X extends WritableStream/ReadableStream/TransformStream`
                    // — `super({ ... })` allocates an underlying stream registry handle and
                    // stashes it on `this` under `__perry_stream_handle__`. Inherited methods
                    // (`pipeTo`, `getWriter`, etc.) and arguments to `pipeTo`/`pipeThrough`
                    // route the receiver through `js_stream_unwrap_handle` at the FFI site
                    // so a subclass instance dispatches to the same FFIs a bare handle does.
                    let stream_kind = match parent_name.as_str() {
                        "ReadableStream" => Some("readable"),
                        "WritableStream" => Some("writable"),
                        "TransformStream" => Some("transform"),
                        _ => None,
                    };
                    if let Some(kind) = stream_kind {
                        let result = lower_stream_super_init(ctx, kind, super_args)?;
                        // Per JS spec field initializers run AFTER super()
                        // returns. Without this, `this.foo = []` declared
                        // on the subclass never executes — instance reads
                        // see uninitialized slots. Mirrors the equivalent
                        // call in the user-class super branch below
                        // (line ~4521). Refs #562.
                        let current_class_name =
                            ctx.class_stack.last().cloned().unwrap_or_default();
                        crate::lower_call::apply_field_initializers_recursive(
                            ctx,
                            &current_class_name,
                            crate::lower_call::FieldInitMode::SelfOnly,
                        )?;
                        return Ok(result);
                    }
                    let node_stream_kind = match parent_name.as_str() {
                        "Readable" => Some("readable"),
                        "Writable" => Some("writable"),
                        "Duplex" => Some("duplex"),
                        "Transform" => Some("transform"),
                        _ => None,
                    };
                    if let Some(kind) = node_stream_kind {
                        let result = lower_node_stream_super_init(ctx, kind, super_args)?;
                        let current_class_name =
                            ctx.class_stack.last().cloned().unwrap_or_default();
                        crate::lower_call::apply_field_initializers_recursive(
                            ctx,
                            &current_class_name,
                            crate::lower_call::FieldInitMode::SelfOnly,
                        )?;
                        return Ok(result);
                    }
                    // `class X extends Map` / `extends Set` — `super(iterable?)`
                    // allocates a real Map/Set backing store, stashes it on
                    // `this` under a hidden field, and installs the collection
                    // method surface (`has`/`get`/`set`/`delete`/`clear`/
                    // `forEach`/`keys`/`values`/`entries`/`size`/`Symbol.iterator`)
                    // so a source-compiled subclass (e.g. NestJS's
                    // `ModulesContainer extends Map`) actually behaves as a Map.
                    // Perry models the instance as a plain object (not a real
                    // exotic Map), so without this `super()` was a no-op and
                    // `m.has(...)` threw "has is not a function".
                    let map_set_kind = match parent_name.as_str() {
                        "Map" => Some(0i32),
                        "Set" => Some(1i32),
                        _ => None,
                    };
                    if let Some(kind) = map_set_kind {
                        let iterable = if let Some(first) = super_args.first() {
                            lower_expr(ctx, first)?
                        } else {
                            double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                        };
                        for a in super_args.iter().skip(1) {
                            let _ = lower_expr(ctx, a)?;
                        }
                        let this_box = match ctx.this_stack.last().cloned() {
                            Some(slot) => ctx.block().load(DOUBLE, &slot),
                            None => double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
                        };
                        ctx.block().call(
                            DOUBLE,
                            "js_map_set_subclass_init",
                            &[
                                (DOUBLE, &this_box),
                                (I32, &kind.to_string()),
                                (DOUBLE, &iterable),
                            ],
                        );
                        let current_class_name =
                            ctx.class_stack.last().cloned().unwrap_or_default();
                        crate::lower_call::apply_field_initializers_recursive(
                            ctx,
                            &current_class_name,
                            crate::lower_call::FieldInitMode::SelfOnly,
                        )?;
                        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                    }
                    // #5137: `class X extends EventEmitter` (node:events) —
                    // `super()` installs the bare EventEmitter listener/emit
                    // surface onto `this` (see `lower_event_emitter_subclass_init`).
                    // `super(opts)` takes an optional options bag in Node; we lower
                    // the args for side effects but the bare emitter seeds no state.
                    if parent_name.as_str() == "EventEmitter" {
                        for a in super_args {
                            let _ = lower_expr(ctx, a)?;
                        }
                        let this_box = match ctx.this_stack.last().cloned() {
                            Some(slot) => ctx.block().load(DOUBLE, &slot),
                            None => double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
                        };
                        lower_event_emitter_subclass_init(ctx, &this_box);
                        let current_class_name =
                            ctx.class_stack.last().cloned().unwrap_or_default();
                        crate::lower_call::apply_field_initializers_recursive(
                            ctx,
                            &current_class_name,
                            crate::lower_call::FieldInitMode::SelfOnly,
                        )?;
                        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                    }
                    // `class X extends Request` / `extends Response`:
                    // `super(input, init)` allocates the underlying native
                    // Web-Fetch handle and stashes its id on `this` under
                    // `__perry_fetch_handle__`. Inherited body methods
                    // (`text`/`json`/…) and property getters route through that
                    // handle at runtime (see `fetch_subclass_handle_id`). This
                    // makes `class Request extends GlobalRequest {}` — exactly
                    // what `@hono/node-server` does — produce a working Request.
                    // `class X extends Event` / `extends CustomEvent` (the `ws`
                    // package's CloseEvent/ErrorEvent/MessageEvent): `super(type,
                    // options)` initializes the standard Event fields/methods onto
                    // `this`. The `X → Event` registry edge (registered at class-
                    // definition time via js_register_class_parent_dynamic) keeps
                    // `instanceof Event` and EventTarget dispatch acceptance.
                    if matches!(parent_name.as_str(), "Event" | "CustomEvent") {
                        let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                        let mut lowered: Vec<String> = Vec::with_capacity(super_args.len());
                        for a in super_args {
                            lowered.push(lower_expr(ctx, a)?);
                        }
                        let arg0 = lowered.first().cloned().unwrap_or_else(|| undef.clone());
                        let arg1 = lowered.get(1).cloned().unwrap_or_else(|| undef.clone());
                        let this_box = match ctx.this_stack.last().cloned() {
                            Some(slot) => ctx.block().load(DOUBLE, &slot),
                            None => undef.clone(),
                        };
                        let argc = super_args.len().min(2).to_string();
                        // `extends CustomEvent` → initialize `constructor` +
                        // `detail` as a CustomEvent, not a plain Event.
                        let is_custom = if parent_name.as_str() == "CustomEvent" {
                            "1"
                        } else {
                            "0"
                        }
                        .to_string();
                        ctx.block().call(
                            DOUBLE,
                            "js_event_subclass_init",
                            &[
                                (DOUBLE, &this_box),
                                (DOUBLE, &arg0),
                                (DOUBLE, &arg1),
                                (I32, &argc),
                                (I32, &is_custom),
                            ],
                        );
                        let current_class_name =
                            ctx.class_stack.last().cloned().unwrap_or_default();
                        crate::lower_call::apply_field_initializers_recursive(
                            ctx,
                            &current_class_name,
                            crate::lower_call::FieldInitMode::SelfOnly,
                        )?;
                        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                    }
                    // `class X extends Promise` — `super(executor)` runs the
                    // ECMA-262 27.2.3.1 Promise constructor against a hidden
                    // backing `Promise` cell stashed on `this`. Inherited
                    // `then`/`catch`/`finally` unwrap that cell (see
                    // `promise::subclass::subclass_backing_promise`), so a
                    // subclass instance behaves as a promise while keeping its
                    // own `constructor`/`instanceof` identity.
                    if parent_name.as_str() == "Promise" {
                        let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                        let mut lowered: Vec<String> = Vec::with_capacity(super_args.len());
                        for a in super_args {
                            lowered.push(lower_expr(ctx, a)?);
                        }
                        let executor = lowered.first().cloned().unwrap_or_else(|| undef.clone());
                        let this_box = match ctx.this_stack.last().cloned() {
                            Some(slot) => ctx.block().load(DOUBLE, &slot),
                            None => undef.clone(),
                        };
                        ctx.block().call(
                            DOUBLE,
                            "js_promise_subclass_init",
                            &[(DOUBLE, &this_box), (DOUBLE, &executor)],
                        );
                        let current_class_name =
                            ctx.class_stack.last().cloned().unwrap_or_default();
                        crate::lower_call::apply_field_initializers_recursive(
                            ctx,
                            &current_class_name,
                            crate::lower_call::FieldInitMode::SelfOnly,
                        )?;
                        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                    }
                    let fetch_subclass_fn = match parent_name.as_str() {
                        "Request" => Some("js_request_subclass_init"),
                        "Response" => Some("js_response_subclass_init"),
                        _ => None,
                    };
                    if let Some(runtime_fn) = fetch_subclass_fn {
                        let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                        let mut lowered: Vec<String> = Vec::with_capacity(super_args.len());
                        for a in super_args {
                            lowered.push(lower_expr(ctx, a)?);
                        }
                        let arg0 = lowered.first().cloned().unwrap_or_else(|| undef.clone());
                        let arg1 = lowered.get(1).cloned().unwrap_or_else(|| undef.clone());
                        let this_box = match ctx.this_stack.last().cloned() {
                            Some(slot) => ctx.block().load(DOUBLE, &slot),
                            None => undef.clone(),
                        };
                        ctx.block().call(
                            DOUBLE,
                            runtime_fn,
                            &[(DOUBLE, &this_box), (DOUBLE, &arg0), (DOUBLE, &arg1)],
                        );
                        // Per JS spec, subclass field initializers run after
                        // super() returns (mirrors the stream/error arms above).
                        let current_class_name =
                            ctx.class_stack.last().cloned().unwrap_or_default();
                        crate::lower_call::apply_field_initializers_recursive(
                            ctx,
                            &current_class_name,
                            crate::lower_call::FieldInitMode::SelfOnly,
                        )?;
                        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                    }
                    // Built-in parent (Error, TypeError, RangeError, etc.)
                    // — user classes extending them need `super(message)` to
                    // assign `this.message = args[0]` and `this.name = parent_name`
                    // so downstream `err.message` / `err.name` access works.
                    // `instanceof Error` walking the extends chain is handled
                    // elsewhere; this just makes `err.message` non-undefined.
                    let is_error_like = matches!(
                        parent_name.as_str(),
                        "Error"
                            | "TypeError"
                            | "RangeError"
                            | "ReferenceError"
                            | "SyntaxError"
                            | "URIError"
                            | "EvalError"
                            | "AggregateError"
                    );
                    // Lower args — at most 1 (message) for Error-like.
                    let mut lowered_args: Vec<String> = Vec::with_capacity(super_args.len());
                    for a in super_args {
                        lowered_args.push(lower_expr(ctx, a)?);
                    }
                    if is_error_like {
                        // Need the `this` pointer to set fields on.
                        let this_slot = ctx.this_stack.last().cloned();
                        if let Some(this_slot) = this_slot {
                            let blk = ctx.block();
                            let this_box = blk.load(DOUBLE, &this_slot);
                            let this_bits = blk.bitcast_double_to_i64(&this_box);
                            let this_handle = blk.and(I64, &this_bits, POINTER_MASK_I64);
                            // this.message = args[0] (if provided)
                            if let Some(msg_val) = lowered_args.first() {
                                let key_idx = ctx.strings.intern("message");
                                let key_handle_global =
                                    format!("@{}", ctx.strings.entry(key_idx).handle_global);
                                let blk = ctx.block();
                                let key_box = blk.load(DOUBLE, &key_handle_global);
                                let key_bits = blk.bitcast_double_to_i64(&key_box);
                                let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                                // Spec: `super(message)` into a built-in Error
                                // sets `message` via DefinePropertyOrThrow with
                                // `{ enumerable: false }` (Test262 NativeError/
                                // *-message), not an enumerable assignment.
                                blk.call_void(
                                    "js_object_set_field_by_name_nonenum",
                                    &[(I64, &this_handle), (I64, &key_raw), (DOUBLE, msg_val)],
                                );
                            }
                            // this.name = <parent_name> as default (can be
                            // overridden by the subclass constructor body).
                            let name_idx = ctx.strings.intern("name");
                            let name_handle_global =
                                format!("@{}", ctx.strings.entry(name_idx).handle_global);
                            let name_val_idx = ctx.strings.intern(&parent_name);
                            let name_val_global =
                                format!("@{}", ctx.strings.entry(name_val_idx).handle_global);
                            let blk = ctx.block();
                            let name_key_box = blk.load(DOUBLE, &name_handle_global);
                            let name_key_bits = blk.bitcast_double_to_i64(&name_key_box);
                            let name_key_raw = blk.and(I64, &name_key_bits, POINTER_MASK_I64);
                            let name_val_box = blk.load(DOUBLE, &name_val_global);
                            blk.call_void(
                                "js_object_set_field_by_name",
                                &[
                                    (I64, &this_handle),
                                    (I64, &name_key_raw),
                                    (DOUBLE, &name_val_box),
                                ],
                            );
                            // #5127: `super(message, options)` must forward the
                            // ES2022 `cause` option. The instance is a generic
                            // object, so install a non-enumerable `cause`
                            // property from args[1] when present.
                            if let Some(opts_val) = lowered_args.get(1) {
                                let blk = ctx.block();
                                blk.call_void(
                                    "js_error_apply_cause_to_object",
                                    &[(I64, &this_handle), (DOUBLE, opts_val)],
                                );
                            }
                        }
                    }
                    return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                }
            };

            // Lower the super-call args.
            let mut lowered_args: Vec<String> = Vec::with_capacity(super_args.len());
            for a in super_args {
                lowered_args.push(lower_expr(ctx, a)?);
            }

            // Inline the parent constructor with the SAME this and a
            // fresh param scope for the parent's params.
            //
            // Walk the parent chain when the IMMEDIATE parent has no
            // constructor of its own — JS spec: an empty class implicitly
            // forwards args to its super, so `class Mid extends Base {}`
            // followed by `class Leaf extends Mid {}` calling `super(...)`
            // must reach Base's constructor body. Without this walk,
            // perry's super() produced a no-op when Mid had no ctor, and
            // Base's `this.config = {...}` never ran. Refs #420 (drizzle
            // PgSerialBuilder → PgColumnBuilder → ColumnBuilder chain
            // where only ColumnBuilder has a ctor body).
            // Walk up the parent chain to find the first class with a
            // local constructor body OR a cross-module ctor stub that must
            // run. JS spec requires `class Mid extends Base {}`
            // followed by `class Leaf extends Mid` calling `super(...)` to
            // reach Base's ctor body (Mid has no ctor → implicit forward).
            // Refs #420 (drizzle's PgSerialBuilder → PgColumnBuilder →
            // ColumnBuilder where only ColumnBuilder has a body).
            //
            // Imported empty-derived classes with no fields still get walked
            // past so their synthesized standalone ctor does not eat forwarded
            // args. Explicit zero-arg ctors and field-initializer ctors stop
            // the walk because their body/initializers must run.
            let mut effective_parent_name = parent_name.clone();
            let mut effective_parent_class = parent_class;
            loop {
                let has_local_body = effective_parent_class.constructor.is_some();
                let has_effectful_imported_ctor = ctx
                    .imported_class_ctors
                    .get(&effective_parent_name)
                    .map(|ctor| ctor.stops_constructor_walk())
                    .unwrap_or(false);
                if has_local_body || has_effectful_imported_ctor {
                    break;
                }
                let Some(grandparent_name) = effective_parent_class
                    .extends_name
                    .as_deref()
                    .map(|s| s.to_string())
                else {
                    break;
                };
                let Some(gp_class) = ctx.classes.get(&grandparent_name).copied() else {
                    break;
                };
                effective_parent_name = grandparent_name;
                effective_parent_class = gp_class;
            }

            if let Some(parent_ctor) = &effective_parent_class.constructor {
                // The parent's synthesized `__perry_cap_*` params (a parent
                // class that captures enclosing locals) are NOT in the
                // user-written `super(...)` args. The CHILD's ctor carries
                // same-named cap params (capture union), bound in the current
                // scope — append their values by NAME so the binder's
                // tail-aligned cap binding sees them. Without this,
                // tail-binding pulled the LAST user arg into the parent's cap
                // slot and the parent ctor's real params read undefined
                // (vendored zod: ZodType's `this._def = def` got undefined).
                let parent_cap_params: Vec<String> = parent_ctor
                    .params
                    .iter()
                    .filter(|p| p.name.starts_with("__perry_cap_"))
                    .map(|p| p.name.clone())
                    .collect();
                if !parent_cap_params.is_empty() {
                    let child_cap_ids: std::collections::HashMap<String, u32> = ctx
                        .class_stack
                        .last()
                        .and_then(|child| ctx.classes.get(child.as_str()))
                        .and_then(|c| c.constructor.as_ref())
                        .map(|ctor| {
                            ctor.params
                                .iter()
                                .filter(|p| p.name.starts_with("__perry_cap_"))
                                .map(|p| (p.name.clone(), p.id))
                                .collect()
                        })
                        .unwrap_or_default();
                    for cap_name in &parent_cap_params {
                        let val = child_cap_ids
                            .get(cap_name)
                            .and_then(|id| ctx.locals.get(id).cloned())
                            .map(|slot| ctx.block().load(DOUBLE, &slot));
                        lowered_args.push(val.unwrap_or_else(|| {
                            double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                        }));
                    }
                }
                // #5437: fill the parent's cap params from its decl-site
                // capture snapshot. The snapshot is authoritative for EVERY
                // parent cap param — `bind_inline_constructor_params` consumes
                // the values the child-forwarding loop appended above only to
                // keep the user/cap tail-split aligned, then discards them in
                // favour of `js_class_capture_value`. This matters when the
                // captured local is out of scope at the super-call site (the
                // forwarded value would be `undefined`); the snapshot still
                // holds the correct decl-site capture. `backfill` sets
                // `caps_absent_from_args=false` because the caps ARE present in
                // `lowered_args` (this is not the caps-absent member-new path).
                let parent_capture_fill = ctx
                    .class_ids
                    .get(effective_parent_name.as_str())
                    .copied()
                    .map(crate::lower_call::CaptureFill::backfill);
                let saved_scope = bind_inline_constructor_params(
                    ctx,
                    &parent_ctor.params,
                    &lowered_args,
                    parent_capture_fill,
                );

                ctx.class_stack.push(effective_parent_name.clone());
                crate::stmt::lower_stmts(ctx, &parent_ctor.body)?;
                ctx.class_stack.pop();

                restore_inline_constructor_scope(ctx, saved_scope);
            } else if let Some(error_kind) = {
                // Issue #573: walk the chain from `effective_parent_class`
                // upward; if it terminates at an Error-like built-in,
                // emit the same Error init the no-parent-class branch
                // does (sets this.message + this.name). Without this,
                // `class C extends Error {}; class D extends C { ctor(m){
                // super(m); } }` reaches here with `effective_parent_class
                // = C` (no own ctor) and a parent of "Error" (not in
                // ctx.classes), so neither inline nor cross-module-ctor
                // path fires and `super(msg)` becomes a no-op.
                let mut found: Option<String> = None;
                let mut cur = Some(effective_parent_name.clone());
                let mut depth = 0usize;
                while let Some(pname) = cur {
                    if matches!(
                        pname.as_str(),
                        "Error"
                            | "TypeError"
                            | "RangeError"
                            | "ReferenceError"
                            | "SyntaxError"
                            | "URIError"
                            | "EvalError"
                            | "AggregateError"
                    ) {
                        found = Some(pname);
                        break;
                    }
                    cur = ctx
                        .classes
                        .get(pname.as_str())
                        .and_then(|c| c.extends_name.clone());
                    depth += 1;
                    if depth > 32 {
                        break;
                    }
                }
                found
            } {
                let this_slot = ctx.this_stack.last().cloned();
                if let Some(this_slot) = this_slot {
                    let blk = ctx.block();
                    let this_box = blk.load(DOUBLE, &this_slot);
                    let this_bits = blk.bitcast_double_to_i64(&this_box);
                    let this_handle = blk.and(I64, &this_bits, POINTER_MASK_I64);
                    if let Some(msg_val) = lowered_args.first() {
                        let key_idx = ctx.strings.intern("message");
                        let key_handle_global =
                            format!("@{}", ctx.strings.entry(key_idx).handle_global);
                        let blk = ctx.block();
                        let key_box = blk.load(DOUBLE, &key_handle_global);
                        let key_bits = blk.bitcast_double_to_i64(&key_box);
                        let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                        // Spec: built-in Error sets `message` non-enumerable.
                        blk.call_void(
                            "js_object_set_field_by_name_nonenum",
                            &[(I64, &this_handle), (I64, &key_raw), (DOUBLE, msg_val)],
                        );
                    }
                    let name_idx = ctx.strings.intern("name");
                    let name_handle_global =
                        format!("@{}", ctx.strings.entry(name_idx).handle_global);
                    let name_val_idx = ctx.strings.intern(&error_kind);
                    let name_val_global =
                        format!("@{}", ctx.strings.entry(name_val_idx).handle_global);
                    let blk = ctx.block();
                    let name_key_box = blk.load(DOUBLE, &name_handle_global);
                    let name_key_bits = blk.bitcast_double_to_i64(&name_key_box);
                    let name_key_raw = blk.and(I64, &name_key_bits, POINTER_MASK_I64);
                    let name_val_box = blk.load(DOUBLE, &name_val_global);
                    blk.call_void(
                        "js_object_set_field_by_name",
                        &[
                            (I64, &this_handle),
                            (I64, &name_key_raw),
                            (DOUBLE, &name_val_box),
                        ],
                    );
                }
            } else if let Some(ctor) = ctx
                .imported_class_ctors
                .get(&effective_parent_name)
                .cloned()
            {
                // Issue #485: parent class is imported (stub with `constructor: None`)
                // and has no inlineable body in this module. Call the cross-module
                // standalone constructor symbol — it exists per-class in the source
                // module (compile_method emits `<source_prefix>__<class>_constructor`)
                // and itself runs `apply_field_initializers_recursive_pub`, so calling
                // it from `super()` inherits the parent's arrow-class-field
                // initializers (e.g. HonoBase's `request = (...) => ...`,
                // `fetch = (...) => ...`) onto `this`. Without this branch, perry
                // silently drops `super(...)` for imported parents and the subclass
                // ends up with only its own fields, breaking hono-base inheritance.
                let undef_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                while lowered_args.len() < ctor.param_count {
                    lowered_args.push(undef_lit.clone());
                }
                let this_slot = ctx.this_stack.last().cloned();
                let this_box = if let Some(slot) = this_slot {
                    ctx.block().load(DOUBLE, &slot)
                } else {
                    undef_lit.clone()
                };
                let ctor_param_types: Vec<crate::types::LlvmType> = std::iter::once(DOUBLE)
                    .chain(lowered_args.iter().map(|_| DOUBLE))
                    .collect();
                let mut ctor_args: Vec<(crate::types::LlvmType, &str)> =
                    Vec::with_capacity(1 + lowered_args.len());
                ctor_args.push((DOUBLE, &this_box));
                for la in &lowered_args {
                    ctor_args.push((DOUBLE, la.as_str()));
                }
                // `super(...)` to an imported parent: the parent ctor's return
                // override does not replace the derived `this`, so discard the
                // return. Declared DOUBLE to match the symbol's real signature
                // (the source standalone ctor returns DOUBLE — see codegen/mod.rs).
                ctx.pending_declares
                    .push((ctor.symbol.clone(), DOUBLE, ctor_param_types));
                let _ = ctx.block().call(DOUBLE, &ctor.symbol, &ctor_args);
            }

            // After the parent body has run (which may have set `this.config`
            // etc.), apply field initializers for each class between
            // `effective_parent_name` (exclusive) and `current_class_name`
            // (inclusive). Per JS spec each default-ctor class's field
            // inits run immediately after that class's super() returns.
            // For drizzle's `SQLiteInteger ← SQLiteBaseInteger ← SQLiteColumn`,
            // walking up from SuperCall in SQLiteInteger finds the
            // inherited ctor at SQLiteColumn (effective_parent_name);
            // SQLiteBaseInteger (intermediate, no ctor) has fields
            // `autoIncrement = this.config.autoIncrement` that must run
            // after SQLiteColumn's body sets `this.config`. Refs #631.
            //
            // Walk parent → ... → effective_parent_name (exclusive),
            // collect intermediate names. Apply SelfOnly for each in
            // root-most-first order, then for current_class_name.
            let mut intermediates: Vec<String> = Vec::new();
            let mut walker = current_class.extends_name.as_deref().map(|s| s.to_string());
            while let Some(pname) = walker {
                if pname == effective_parent_name {
                    break;
                }
                intermediates.push(pname.clone());
                walker = ctx
                    .classes
                    .get(&pname)
                    .and_then(|c| c.extends_name.as_deref().map(|s| s.to_string()));
            }
            // Root-most intermediate first (reverse insertion order).
            intermediates.reverse();
            for inter in &intermediates {
                crate::lower_call::apply_field_initializers_recursive(
                    ctx,
                    inter,
                    crate::lower_call::FieldInitMode::SelfOnly,
                )?;
            }
            crate::lower_call::apply_field_initializers_recursive(
                ctx,
                &current_class_name,
                crate::lower_call::FieldInitMode::SelfOnly,
            )?;

            // super() evaluates to undefined in JS.
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }

        // -------- isNaN(x) — global, coerces via ToNumber --------
        // Per ECMA-262 §19.2.3, the global `isNaN` first coerces its
        // argument via ToNumber and then checks if the result is NaN.
        // The pre-fix inline `fcmp uno x, x` idiom checked the raw bit
        // pattern, but every NaN-boxed value (strings, pointers, etc.)
        // has a NaN bit pattern — `isNaN("1")` returned true (correct
        // is false because "1" coerces to 1). Route to `js_is_nan` which
        // implements the ToNumber-then-check sequence. `Number.isNaN`
        // (strict, no coercion) goes through `Expr::NumberIsNaN` and
        // already calls `js_number_is_nan`.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
