//! IsNaN..MapNew: Math/Map/Set/WebAssembly/JsonStringify helpers.
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
        Expr::IsNaN(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "js_is_nan", &[(DOUBLE, &v)]))
        }

        // -------- Math.pow (special variant — separate from Binary::Pow) --------
        Expr::MathPow(base, exp) => {
            let b = lower_expr(ctx, base)?;
            let e = lower_expr(ctx, exp)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_math_pow", &[(DOUBLE, &b), (DOUBLE, &e)]))
        }

        // -------- Math.imul — 32-bit wrapping integer multiply --------
        // ECMAScript: `Math.imul(a, b) = (ToInt32(a) * ToInt32(b)) | 0`.
        // ToInt32 on a finite double is "truncate to i64 (wrapping), then
        // take the low 32 bits", which is exactly what `fptosi f64 → i64`
        // followed by `trunc i64 → i32` produces. LLVM `mul i32` wraps
        // without `nsw`/`nuw`, giving the required 32-bit overflow. Result
        // re-boxes via `sitofp` so the JS-visible value is a signed i32 in
        // a double (e.g. -2110866647 for the FNV-1a constants in the #40
        // repro). This unblocks every hash (FNV-1a-32, MurmurHash3, xxhash,
        // CRC32) and PRNG (PCG, xorshift*) that uses the canonical
        // 32-bit-wrap spelling instead of the 16-bit hi/lo workaround.
        // NaN/Inf inputs coerce to 0 in spec JS; `fptosi` saturates instead,
        // but no real hash/PRNG feeds those to imul, so we accept that minor
        // divergence rather than adding a compare-and-select gate per call.
        Expr::MathImul(a, b) => {
            let av = lower_expr(ctx, a)?;
            let bv = lower_expr(ctx, b)?;
            let blk = ctx.block();
            let a_i64 = blk.fptosi(DOUBLE, &av, I64);
            let b_i64 = blk.fptosi(DOUBLE, &bv, I64);
            let a_i32 = blk.trunc(I64, &a_i64, I32);
            let b_i32 = blk.trunc(I64, &b_i64, I32);
            let prod = blk.mul(I32, &a_i32, &b_i32);
            Ok(blk.sitofp(I32, &prod, DOUBLE))
        }

        // -------- new Error() / new Error(message) --------
        Expr::ErrorNew(opt_msg) => {
            if let Some(msg_expr) = opt_msg {
                let msg = lower_expr(ctx, msg_expr)?;
                let blk = ctx.block();
                let err_handle = blk.call(I64, "js_error_new_from_value", &[(DOUBLE, &msg)]);
                Ok(nanbox_pointer_inline(blk, &err_handle))
            } else {
                let err_handle = ctx.block().call(I64, "js_error_new", &[]);
                Ok(nanbox_pointer_inline(ctx.block(), &err_handle))
            }
        }

        // -------- arr.pop() / arr.shift() (special HIR variants) --------
        // Like ArrayPush, the HIR pre-resolves these so we get the
        // local id directly. Pop returns the removed element (NaN if
        // empty); shift removes from the front. We currently support
        // pop only.
        Expr::ArrayPop(array_id) => {
            // pop is a read-only access for the storage; we don't need
            // to write back. Resolve via LocalGet so closure captures
            // and module globals work transparently.
            let arr_box = lower_expr(ctx, &Expr::LocalGet(*array_id))?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            Ok(blk.call(DOUBLE, "js_array_pop_f64", &[(I64, &arr_handle)]))
        }

        // -------- arr.map(callback) (special variant) --------
        // The runtime js_array_map takes a closure header pointer and
        // calls it for each element. The callback expression usually
        // lowers to a NaN-boxed closure value, which we unbox to i64.
        Expr::ArrayMap { array, callback } => {
            let arr_box = lower_expr(ctx, array)?;
            let cb_box = lower_expr(ctx, callback)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            let result = blk.call(
                I64,
                "js_array_map",
                &[(I64, &arr_handle), (I64, &cb_handle)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- map.set(key, value) / .get / .has --------
        Expr::MapSet { map, key, value } => {
            let m_box = lower_expr(ctx, map)?;
            let k_box = lower_expr(ctx, key)?;
            let v_box = lower_expr(ctx, value)?;
            let blk = ctx.block();
            let m_handle = unbox_to_i64(blk, &m_box);
            let new_handle = blk.call(
                I64,
                "js_map_set",
                &[(I64, &m_handle), (DOUBLE, &k_box), (DOUBLE, &v_box)],
            );
            // map.set returns the (possibly-realloc'd) map. Re-NaN-box
            // and return. The caller may need to write this back to a
            // local; that's the caller's problem if Map is held in a
            // mutable variable that grows.
            Ok(nanbox_pointer_inline(blk, &new_handle))
        }
        Expr::MapGet { map, key } => {
            let m_box = lower_expr(ctx, map)?;
            let k_box = lower_expr(ctx, key)?;
            let blk = ctx.block();
            let m_handle = unbox_to_i64(blk, &m_box);
            Ok(blk.call(DOUBLE, "js_map_get", &[(I64, &m_handle), (DOUBLE, &k_box)]))
        }
        Expr::MapHas { map, key } => {
            let m_box = lower_expr(ctx, map)?;
            let k_box = lower_expr(ctx, key)?;
            let blk = ctx.block();
            let m_handle = unbox_to_i64(blk, &m_box);
            let i32_v = blk.call(I32, "js_map_has", &[(I64, &m_handle), (DOUBLE, &k_box)]);
            // NaN-tagged boolean for "true"/"false" printing.
            let bit = blk.icmp_ne(I32, &i32_v, "0");
            let tagged = blk.select(
                crate::types::I1,
                &bit,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            Ok(blk.bitcast_i64_to_double(&tagged))
        }

        // -------- Math.* unary helpers (Phase B.15) --------
        // Math.* unary functions: use LLVM intrinsics directly so the
        // generated code becomes a single hardware instruction (or
        // libm call resolved at link time, which is always present).
        // Avoids depending on `js_math_*` runtime symbols which the
        // auto-optimizer's dead-stripping was removing from the
        // built `libperry_runtime.a`.
        //
        // Uses LLVM intrinsics (llvm.sqrt.f64, llvm.floor.f64, etc.).
        Expr::MathSqrt(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "llvm.sqrt.f64", &[(DOUBLE, &v)]))
        }
        Expr::MathFloor(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "llvm.floor.f64", &[(DOUBLE, &v)]))
        }
        Expr::MathCeil(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "llvm.ceil.f64", &[(DOUBLE, &v)]))
        }
        Expr::MathRound(operand) => {
            // JS Math.round: round-half-toward-positive-infinity. We
            // emulate via floor(x + 0.5) then fcopysign to preserve -0.
            let v = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            let half = blk.fadd(&v, "0.5");
            let floored = blk.call(DOUBLE, "llvm.floor.f64", &[(DOUBLE, &half)]);
            Ok(blk.call(
                DOUBLE,
                "llvm.copysign.f64",
                &[(DOUBLE, &floored), (DOUBLE, &v)],
            ))
        }
        Expr::MathAbs(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "llvm.fabs.f64", &[(DOUBLE, &v)]))
        }
        Expr::MathLog(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "llvm.log.f64", &[(DOUBLE, &v)]))
        }
        Expr::MathLog2(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "llvm.log2.f64", &[(DOUBLE, &v)]))
        }
        Expr::MathLog10(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "llvm.log10.f64", &[(DOUBLE, &v)]))
        }
        Expr::MathLog1p(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "js_math_log1p", &[(DOUBLE, &v)]))
        }
        // Math.random — return 0.5 sentinel. Real impl needs a PRNG
        // we'd link in; sentinel keeps the compile-pass count up.
        Expr::MathRandom => Ok(ctx.block().call(DOUBLE, "js_math_random", &[])),

        // ── WebAssembly host (issue #76) ──────────────────────────────
        // The runtime shims (perry-runtime/src/webassembly.rs) handle
        // bytes extraction, instance handles, and error reporting. The
        // wasmi engine itself is in the optional `perry-wasm-host`
        // crate, only linked when the user passes
        // `--enable-wasm-runtime`. Programs that never call these
        // builtins never reference the runtime shims, so the linker
        // dead-strips them and `perry_wasm_host_*` is never demanded.
        Expr::WebAssemblyValidate(bytes) => {
            let v = lower_expr(ctx, bytes)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_webassembly_validate", &[(DOUBLE, &v)]))
        }
        Expr::WebAssemblyCompile(bytes) => {
            let v = lower_expr(ctx, bytes)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_webassembly_compile", &[(DOUBLE, &v)]))
        }
        Expr::WebAssemblyModuleNew(bytes) => {
            let v = lower_expr(ctx, bytes)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_webassembly_module_new", &[(DOUBLE, &v)]))
        }
        Expr::WebAssemblyModuleExports(module) => {
            let v = lower_expr(ctx, module)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_webassembly_module_exports", &[(DOUBLE, &v)]))
        }
        Expr::WebAssemblyModuleImports(module) => {
            let v = lower_expr(ctx, module)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_webassembly_module_imports", &[(DOUBLE, &v)]))
        }
        Expr::WebAssemblyModuleCustomSections { module, name } => {
            let module_v = lower_expr(ctx, module)?;
            let name_v = lower_expr(ctx, name)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_webassembly_module_custom_sections",
                &[(DOUBLE, &module_v), (DOUBLE, &name_v)],
            ))
        }
        Expr::WebAssemblyInstantiate(bytes) => {
            let v = lower_expr(ctx, bytes)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_webassembly_instantiate", &[(DOUBLE, &v)]))
        }
        Expr::WebAssemblyCallExport {
            instance,
            name,
            args,
        } => {
            let inst = lower_expr(ctx, instance)?;
            let name_v = lower_expr(ctx, name)?;
            let lowered_args: Vec<String> = args
                .iter()
                .map(|a| lower_expr(ctx, a))
                .collect::<Result<Vec<_>>>()?;
            let blk = ctx.block();
            match lowered_args.len() {
                0 => Ok(blk.call(
                    DOUBLE,
                    "js_webassembly_call_export_0",
                    &[(DOUBLE, &inst), (DOUBLE, &name_v)],
                )),
                1 => Ok(blk.call(
                    DOUBLE,
                    "js_webassembly_call_export_1",
                    &[
                        (DOUBLE, &inst),
                        (DOUBLE, &name_v),
                        (DOUBLE, &lowered_args[0]),
                    ],
                )),
                2 => Ok(blk.call(
                    DOUBLE,
                    "js_webassembly_call_export_2",
                    &[
                        (DOUBLE, &inst),
                        (DOUBLE, &name_v),
                        (DOUBLE, &lowered_args[0]),
                        (DOUBLE, &lowered_args[1]),
                    ],
                )),
                3 => Ok(blk.call(
                    DOUBLE,
                    "js_webassembly_call_export_3",
                    &[
                        (DOUBLE, &inst),
                        (DOUBLE, &name_v),
                        (DOUBLE, &lowered_args[0]),
                        (DOUBLE, &lowered_args[1]),
                        (DOUBLE, &lowered_args[2]),
                    ],
                )),
                _ => Ok(blk.call(
                    DOUBLE,
                    "js_webassembly_call_export_4",
                    &[
                        (DOUBLE, &inst),
                        (DOUBLE, &name_v),
                        (DOUBLE, &lowered_args[0]),
                        (DOUBLE, &lowered_args[1]),
                        (DOUBLE, &lowered_args[2]),
                        (DOUBLE, &lowered_args[3]),
                    ],
                )),
            }
        }

        // `JSON.stringify(value, replacer, indent)` — full form via
        // runtime `js_json_stringify_full` which handles array/function
        // replacers, indent spaces, circular detection (throws
        // TypeError), and `toJSON`.
        Expr::JsonStringifyFull(value, replacer, indent) => {
            let v = lower_expr(ctx, value)?;
            let r = lower_expr(ctx, replacer)?;
            let i = lower_expr(ctx, indent)?;
            let blk = ctx.block();
            let result_i64 = blk.call(
                I64,
                "js_json_stringify_full",
                &[(DOUBLE, &v), (DOUBLE, &r), (DOUBLE, &i)],
            );
            Ok(blk.bitcast_i64_to_double(&result_i64))
        }

        // `new Map()` — alloc with default capacity 8 (the runtime grows
        // as needed). Result is NaN-boxed with POINTER_TAG.
        Expr::MapNew => {
            let cap = "8".to_string();
            let handle = ctx.block().call(I64, "js_map_alloc", &[(I32, &cap)]);
            Ok(nanbox_pointer_inline(ctx.block(), &handle))
        }

        // -------- Logical operators (Phase B.6) --------
        // `a && b` and `a || b` short-circuit. We compile `a` first, branch
        // on its truthiness (treating 0.0 as false / non-zero as true),
        // and either evaluate `b` or jump straight to the merge with `a`'s
        // value. The merge block uses a phi to pick the right result.
        // `??` (Coalesce) requires NaN-tag inspection (null/undefined
        // checks), so it lands in a later slice.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
