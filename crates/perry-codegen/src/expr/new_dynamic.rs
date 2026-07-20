//! New / NewDynamic.
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
use crate::native_value::MaterializationReason;
#[allow(unused_imports)]
use crate::type_analysis::{
    compute_auto_captures, is_array_expr, is_bigint_expr, is_bool_expr, is_map_expr,
    is_numeric_expr, is_set_expr, is_string_expr, is_url_search_params_expr, receiver_class_name,
};
#[allow(unused_imports)]
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

#[allow(unused_imports)]
use super::{
    buffer_alias_metadata_suffix, can_lower_expr_as_i32, downgrade_buffer_aliases_in_expr,
    emit_layout_note_slot_on_block, emit_shadow_slot_clear, emit_shadow_slot_update_for_expr,
    emit_string_literal_global, emit_v8_export_call, emit_v8_member_method_call,
    emit_write_barrier, emit_write_barrier_slot_on_block, expr_is_known_non_pointer_shadow_value,
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

/// A `new` callee that is a primitive literal — never a constructor, so
/// `new <it>(…)` is a `TypeError`. Covers number / bool / null / undefined /
/// string / bigint literals (the cases the runtime construct path can't always
/// tag-reject, notably `f64` numbers) and regex literals (`new /z/()` is a
/// TypeError: a RegExp instance has no [[Construct]] internal method,
/// test262 S15.10.7_A2_T1 / S15.10.7_A2_T2).
fn new_callee_is_primitive_literal(callee: &Expr) -> bool {
    matches!(
        callee,
        Expr::Integer(_)
            | Expr::Number(_)
            | Expr::Bool(_)
            | Expr::Null
            | Expr::Undefined
            | Expr::String(_)
            | Expr::WtfString(_)
            | Expr::BigInt(_)
            | Expr::RegExp { .. }
    )
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::New {
            class_name,
            args,
            byte_offset,
            cap_args_appended,
            ..
        } => {
            // #5253: under `--debug-symbols`, attach this `new`'s source
            // `file:line` so a "X is not a constructor" throw from `lower_new`'s
            // runtime-construct fallback (or a built-in non-constructor) renders
            // a location. No-op for resolved user classes (no throw fires).
            crate::expr::calls::emit_call_location_at(ctx, *byte_offset);
            lower_new(ctx, class_name, args, *cap_args_appended)
        }

        // `new <callee>(...spread)` — spread-bearing construction. Fold every
        // argument (regular pushed, spread sources expanded via
        // `js_array_like_to_array` + concat) into a single JS array in
        // evaluation order, then dispatch through `js_new_function_construct_apply`
        // which materialises a flat buffer and reuses the full callee-shape
        // dispatch of the non-spread `js_new_function_construct`.
        Expr::NewDynamicSpread {
            callee,
            args,
            byte_offset,
        } => {
            use perry_hir::CallArg;
            let new_byte_offset = *byte_offset;
            downgrade_buffer_aliases_in_expr(ctx, callee, MaterializationReason::UnknownCallEscape);
            for arg in args {
                match arg {
                    CallArg::Expr(expr) | CallArg::Spread(expr) => {
                        downgrade_buffer_aliases_in_expr(
                            ctx,
                            expr,
                            MaterializationReason::UnknownCallEscape,
                        )
                    }
                }
            }
            let func_double = lower_expr(ctx, callee)?;
            let mut acc_handle = ctx.block().call(I64, "js_array_alloc", &[(I32, "0")]);
            for a in args {
                match a {
                    CallArg::Expr(e) => {
                        let v = lower_expr(ctx, e)?;
                        acc_handle = ctx.block().call(
                            I64,
                            "js_array_push_f64",
                            &[(I64, &acc_handle), (DOUBLE, &v)],
                        );
                    }
                    CallArg::Spread(e) => {
                        let part_box = lower_expr(ctx, e)?;
                        let part_handle =
                            ctx.block()
                                .call(I64, "js_array_like_to_array", &[(DOUBLE, &part_box)]);
                        acc_handle = ctx.block().call(
                            I64,
                            "js_array_concat",
                            &[(I64, &acc_handle), (I64, &part_handle)],
                        );
                    }
                }
            }
            let args_box = nanbox_pointer_inline(ctx.block(), &acc_handle);
            // #5253: locate the not-a-constructor throw the apply path can raise.
            crate::expr::calls::emit_call_location_at(ctx, new_byte_offset);
            let result = ctx.block().call(
                DOUBLE,
                "js_new_function_construct_apply",
                &[(DOUBLE, &func_double), (DOUBLE, &args_box)],
            );
            // Write-back: when the callee is a statically-known user class,
            // propagate constructor mutations (e.g. `++called`) back to the
            // outer captured locals. The runtime construction path stores
            // mutations to `inst.__perry_cap_*` but cannot reach the caller's
            // outer alloca slots directly.
            // Also handles `LocalGet(id)` where the local was assigned a
            // ClassRef (e.g. `const ctor = C; new (ctor as any)(...args)`).
            let class_name: Option<String> = match callee.as_ref() {
                Expr::ClassRef(cn) => Some(cn.clone()),
                Expr::LocalGet(id) => ctx
                    .local_id_to_name
                    .get(id)
                    .and_then(|name| ctx.local_class_aliases.get(name))
                    .cloned(),
                _ => None,
            };
            if let Some(cn) = class_name {
                if let Some(class) = ctx.classes.get(cn.as_str()).copied() {
                    let bits = ctx.block().bitcast_double_to_i64(&result);
                    let inst_handle = ctx.block().and(I64, &bits, POINTER_MASK_I64);
                    crate::lower_call::emit_class_capture_writeback(ctx, class, &inst_handle, &[]);
                }
            }
            Ok(result)
        }

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
        Expr::NewDynamic {
            callee,
            args,
            byte_offset,
        } => {
            // #5253: source location of this `new` for the not-a-constructor
            // throws below. `const X: any = undefined; new X()` lowers here
            // (callee `LocalGet`), so this is what localizes ajv's
            // `undefined is not a constructor`.
            let new_byte_offset = *byte_offset;
            // `new <primitive-literal>(…)` is always a `TypeError` — a primitive
            // is never a constructor (`new 1`, `new 1.5`, `new true`, `new null`,
            // `new undefined`, `new "s"`). Number literals lower to a plain `f64`
            // whose bit pattern overlaps the raw-pointer encoding, so the runtime
            // construct path can't tag-distinguish them; handle every primitive
            // literal here for a uniform, deterministic throw. Args are lowered
            // first for their side effects (spec evaluation order).
            if new_callee_is_primitive_literal(callee.as_ref()) {
                let _ = lower_expr(ctx, callee)?;
                for a in args {
                    let _ = lower_expr(ctx, a)?;
                }
                crate::expr::calls::emit_call_location_at(ctx, new_byte_offset);
                return Ok(ctx.block().call(DOUBLE, "js_throw_not_a_constructor", &[]));
            }

            // Case 1 + 2: callee is statically a class.
            //
            // #5437: this is a NewDynamic-routed construct (`new (Foo)()`,
            // `new ns.Foo()` for a namespace import) — the bare-`ast::Expr::Ident`
            // HIR arm that appends class captures was NOT taken, so the captures
            // are absent from `args`. Route to `lower_new_member_captured` so a
            // function-nested capturing class fills its `__perry_cap_*` ctor
            // params from the decl-site snapshot. No-op for non-capturing
            // classes (no cap params to fill).
            if let Some(name) = try_static_class_name(callee.as_ref(), ctx) {
                return crate::lower_call::lower_new_member_captured(ctx, name, args);
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
            if let Expr::PropertyGet {
                object, property, ..
            } = callee.as_ref()
            {
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
            if let Expr::PropertyGet {
                object, property, ..
            } = callee.as_ref()
            {
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
            if let Expr::PropertyGet {
                object, property, ..
            } = callee.as_ref()
            {
                if matches!(property.as_str(), "BlockList" | "SocketAddress") {
                    if let Expr::NativeModuleRef(mod_name) = object.as_ref() {
                        if mod_name == "net" || mod_name == "node:net" {
                            // NewDynamic reroute of a native-module builtin ctor
                            // export: no HIR cap forwards are appended here.
                            return lower_new(ctx, property, args, 0);
                        }
                    }
                }
            }

            if let Expr::PropertyGet {
                object, property, ..
            } = callee.as_ref()
            {
                if property == "WebSocket" {
                    if let Expr::NativeModuleRef(mod_name) = object.as_ref() {
                        if mod_name == "http" || mod_name == "node:http" {
                            // NewDynamic reroute of a native-module builtin ctor
                            // export: no HIR cap forwards are appended here.
                            return lower_new(ctx, property, args, 0);
                        }
                    }
                }
            }

            // `new crypto.Certificate()` is a legacy constructor in Node, but
            // the implementation is a stateless namespace over the same SPKAC
            // helper methods as `crypto.Certificate.*`. Represent instances as
            // the `crypto.Certificate` native namespace so method calls dispatch
            // through the existing native-module path.
            if let Expr::PropertyGet {
                object, property, ..
            } = callee.as_ref()
            {
                if property == "Certificate" && args.is_empty() {
                    if let Expr::NativeModuleRef(mod_name) = object.as_ref() {
                        if mod_name == "crypto" {
                            let module_name = "crypto.Certificate";
                            let mod_idx = ctx.strings.intern(module_name);
                            let mod_bytes_global =
                                format!("@{}", ctx.strings.entry(mod_idx).bytes_global);
                            let mod_len_str = module_name.len().to_string();
                            let install_sym = crate::nm_install::nm_install_symbol(module_name);
                            let blk = ctx.block();
                            if let Some(s) = install_sym {
                                blk.call_void(s, &[]);
                            }
                            return Ok(blk.call(
                                DOUBLE,
                                "js_create_native_module_namespace",
                                &[(PTR, &mod_bytes_global), (I64, &mod_len_str)],
                            ));
                        }
                    }
                }
            }

            // `new crypto.DiffieHellman(...)` /
            // `new crypto.DiffieHellmanGroup(name)` are legacy constructor
            // aliases for the existing classic-DH factory helpers.
            if let Expr::PropertyGet {
                object, property, ..
            } = callee.as_ref()
            {
                if matches!(property.as_str(), "DiffieHellman" | "DiffieHellmanGroup") {
                    if let Expr::NativeModuleRef(mod_name) = object.as_ref() {
                        if mod_name == "crypto" {
                            if property == "DiffieHellmanGroup" {
                                let group = if let Some(arg) = args.first() {
                                    lower_expr(ctx, arg)?
                                } else {
                                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                                };
                                return Ok(ctx.block().call(
                                    DOUBLE,
                                    "js_crypto_get_diffie_hellman",
                                    &[(DOUBLE, &group)],
                                ));
                            }

                            let first = if let Some(arg) = args.first() {
                                lower_expr(ctx, arg)?
                            } else {
                                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                            };
                            let second = if let Some(arg) = args.get(1) {
                                lower_expr(ctx, arg)?
                            } else {
                                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                            };
                            let third = if let Some(arg) = args.get(2) {
                                lower_expr(ctx, arg)?
                            } else {
                                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                            };
                            return Ok(ctx.block().call(
                                DOUBLE,
                                "js_crypto_create_diffie_hellman",
                                &[(DOUBLE, &first), (DOUBLE, &second), (DOUBLE, &third)],
                            ));
                        }
                    }
                }
            }

            // `new v8.GCProfiler()` (#3142) — allocate a fresh native-module
            // instance whose `start()` / `stop()` methods dispatch through the
            // runtime native-module method table.
            if let Expr::PropertyGet {
                object, property, ..
            } = callee.as_ref()
            {
                if property == "GCProfiler" {
                    if let Expr::NativeModuleRef(mod_name) = object.as_ref() {
                        if mod_name == "v8" {
                            for a in args {
                                let _ = lower_expr(ctx, a)?;
                            }
                            // #3142: the profiler's `start()`/`stop()` instance
                            // methods dispatch through the generic native-module
                            // registry (`dispatch_native_module_method` →
                            // `("v8.GCProfiler", …)`), which only resolves once
                            // `js_nm_install_v8()` has populated the V8 dispatch
                            // slot. A program whose other `v8.*` calls are all
                            // direct-lowered never emits that install, so without
                            // this `p.start()`/`p.stop()` fall through to
                            // `undefined`. Emit it here (mirrors the sibling
                            // `crypto.Certificate` new-branch).
                            if let Some(s) = crate::nm_install::nm_install_symbol("v8.GCProfiler") {
                                ctx.block().call_void(s, &[]);
                            }
                            return Ok(ctx.block().call(DOUBLE, "js_v8_gc_profiler_new", &[]));
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
            if let Expr::PropertyGet {
                object, property, ..
            } = callee.as_ref()
            {
                if let Expr::NativeModuleRef(mod_name) = object.as_ref() {
                    if mod_name == "stream"
                        && matches!(
                            property.as_str(),
                            "Readable" | "Writable" | "Duplex" | "Transform" | "PassThrough"
                        )
                    {
                        // NewDynamic reroute of a native-module stream ctor
                        // export: no HIR cap forwards are appended here.
                        return lower_new(ctx, property, args, 0);
                    }
                }
            }

            // `new v8.Serializer()` / `new v8.Deserializer(buf)` (and the
            // `Default*` subclasses) (#3680) — route to the runtime
            // constructors that allocate a codec-backed instance object whose
            // methods dispatch through the native-module method table.
            if let Expr::PropertyGet {
                object, property, ..
            } = callee.as_ref()
            {
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
            if let Expr::PropertyGet {
                object, property, ..
            } = callee.as_ref()
            {
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
            //
            // #5437: this is a MEMBER-callee construct — the class's
            // captures are NOT appended at this `new` site (the captured
            // enclosing local is out of scope here), so route to
            // `lower_new_member_captured` which fills the synthesized
            // `__perry_cap_*` ctor params from the class's decl-site capture
            // snapshot instead of binding them to `undefined`.
            if let Expr::PropertyGet {
                object, property, ..
            } = callee.as_ref()
            {
                if let Expr::LocalGet(obj_id) = object.as_ref() {
                    if let Some(class_name) = ctx
                        .local_class_field_aliases
                        .get(obj_id)
                        .and_then(|f| f.get(property))
                        .cloned()
                    {
                        return crate::lower_call::lower_new_member_captured(
                            ctx,
                            &class_name,
                            args,
                        );
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
                    byte_offset: new_byte_offset,
                };
                let else_synth = Expr::NewDynamic {
                    callee: else_expr.clone(),
                    args: args.clone(),
                    byte_offset: new_byte_offset,
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
            // Also route PropertyGet / IndexGet callees through `js_new_function_construct`:
            // covers `new date.constructor(value)` (date-fns
            // `constructFrom`) and generic `new obj.factory(...)` shapes
            // where `obj.factory` or `ctors[i]` resolves to a closure pointer at
            // runtime. The runtime helper detects the global Date /
            // Array / Object thunks and dispatches into the matching
            // real factory; non-matching closures still get the
            // class_id=0 empty-object baseline.
            // `Expr::Logical` covers `new (A ?? B)()` / `new (A || B)()` /
            // `new (A && B)()` — picking a constructor with a short-circuit
            // operator. zod v4's `safeParse` builds its error via
            // `new (_Err ?? errors.$ZodError)(issues)` (#4699): without this
            // the callee fell through to the Case-4 empty-object placeholder,
            // so the `ZodError` constructor never ran and `r.error.issues`
            // was `undefined`. The whole logical expression lowers to a single
            // closure value, which `js_new_function_construct` handles exactly
            // like a `LocalGet` callee (and still falls back to the class_id=0
            // empty object if the value turns out non-callable).
            let routes_through_function_construct = matches!(
                callee.as_ref(),
                Expr::FuncRef(_)
                    | Expr::ExternFuncRef { .. }
                    | Expr::LocalGet(_)
                    | Expr::PropertyGet { .. }
                    | Expr::IndexGet { .. }
                    | Expr::Closure { .. }
                    | Expr::Logical { .. }
            );
            if routes_through_function_construct {
                downgrade_buffer_aliases_in_expr(
                    ctx,
                    callee,
                    MaterializationReason::UnknownCallEscape,
                );
                for arg in args {
                    downgrade_buffer_aliases_in_expr(
                        ctx,
                        arg,
                        MaterializationReason::UnknownCallEscape,
                    );
                }
                let func_double = lower_expr(ctx, callee)?;
                let lowered_args: Vec<String> = args
                    .iter()
                    .map(|a| lower_expr(ctx, a))
                    .collect::<Result<Vec<_>>>()?;
                let (args_ptr, args_len) = lower_js_args_array(ctx, &lowered_args);
                // #5253: locate a not-a-constructor throw from the runtime
                // construct path (a `LocalGet` callee holding `undefined`, a
                // non-callable value, etc.).
                crate::expr::calls::emit_call_location_at(ctx, new_byte_offset);
                let result = ctx.block().call(
                    DOUBLE,
                    "js_new_function_construct",
                    &[(DOUBLE, &func_double), (PTR, &args_ptr), (I64, &args_len)],
                );
                return Ok(result);
            }

            // Case 4: generic fallback — route any remaining callee shape
            // through `js_new_function_construct`. This is what makes
            // `new <primitive>` (`new 1`, `new true`, `new null`) and
            // `new <boxed-wrapper>` (`new new Boolean(true)`) throw the spec
            // `TypeError`: the runtime inspects the NaN-box tag / boxed payload
            // and rejects non-constructors. Unknown closure values still fall
            // back to the class_id=0 empty-object baseline inside the helper,
            // preserving the previous best-effort behavior for shapes the
            // compiler can't resolve statically.
            downgrade_buffer_aliases_in_expr(ctx, callee, MaterializationReason::UnknownCallEscape);
            for arg in args {
                downgrade_buffer_aliases_in_expr(
                    ctx,
                    arg,
                    MaterializationReason::UnknownCallEscape,
                );
            }
            let func_double = lower_expr(ctx, callee)?;
            let lowered_args: Vec<String> = args
                .iter()
                .map(|a| lower_expr(ctx, a))
                .collect::<Result<Vec<_>>>()?;
            let (args_ptr, args_len) = lower_js_args_array(ctx, &lowered_args);
            // #5253: locate the not-a-constructor throw for `new <primitive>` /
            // `new <non-constructor-value>` rejected inside the runtime helper.
            crate::expr::calls::emit_call_location_at(ctx, new_byte_offset);
            let result = ctx.block().call(
                DOUBLE,
                "js_new_function_construct",
                &[(DOUBLE, &func_double), (PTR, &args_ptr), (I64, &args_len)],
            );
            Ok(result)
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
