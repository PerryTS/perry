//! Issue #636: namespace member call —
//! `Call { callee: PropertyGet { ExternFuncRef(ns), method }, args }`
//! where `ns ∈ namespace_imports`.

use anyhow::{bail, Result};
use perry_hir::Expr;

use crate::expr::{lower_expr, nanbox_pointer_inline, unbox_to_i64, FnCtx};
use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I32, I64, PTR};

pub fn try_lower_namespace_member_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    // Issue #636: namespace member call —
    // `Call { callee: PropertyGet { ExternFuncRef(ns), method }, args }`
    // where `ns ∈ namespace_imports`. Pre-fix this fell through to the
    // generic method-dispatch path which lower_expr'd the namespace as
    // its TAG_TRUE/stub-object value and then did `js_native_call_method`
    // with `method` against a non-callable receiver — TypeError or
    // silent 0 return.
    //
    // Resolution: route to the source's exported `method`. If `method`
    // is a var (let/const-bound closure — the canonical
    // `export const make = (s) => ...` shape), fetch the closure value
    // via the zero-arg getter `perry_fn_<src>__<method>()` and invoke
    // through `js_closure_callN`. If it's a function declaration
    // (`export function make(s)`), call the symbol directly with rest
    // bundling — same as the existing FuncRef path.
    let Expr::PropertyGet { object, property } = callee else {
        return Ok(None);
    };
    let Expr::ExternFuncRef { name: ns_name, .. } = object.as_ref() else {
        return Ok(None);
    };
    if !ctx.namespace_imports.contains(ns_name) {
        return Ok(None);
    }
    if ctx
        .namespace_node_submodules
        .get(ns_name)
        .is_some_and(|submod| submod == "timers")
    {
        match property.as_str() {
            "setTimeout" if !args.is_empty() => {
                let cb_box = lower_expr(ctx, &args[0])?;
                let delay_box = if args.len() >= 2 {
                    lower_expr(ctx, &args[1])?
                } else {
                    double_literal(0.0)
                };
                let blk = ctx.block();
                let cb_handle = unbox_to_i64(blk, &cb_box);
                if args.len() <= 2 {
                    let id = blk.call(
                        I64,
                        "js_set_timeout_callback",
                        &[(I64, &cb_handle), (DOUBLE, &delay_box)],
                    );
                    return Ok(Some(nanbox_pointer_inline(blk, &id)));
                }
                let n = args.len() - 2;
                let buf = ctx.func.alloca_entry_array(DOUBLE, n);
                for (i, a) in args.iter().skip(2).enumerate() {
                    let v = lower_expr(ctx, a)?;
                    let blk = ctx.block();
                    let slot = blk.gep(DOUBLE, &buf, &[(I64, &format!("{}", i))]);
                    blk.store(DOUBLE, &v, &slot);
                }
                let ptr_reg = ctx.block().next_reg();
                ctx.block().emit_raw(format!(
                    "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                    ptr_reg, n, buf
                ));
                let blk = ctx.block();
                let id = blk.call(
                    I64,
                    "js_set_timeout_callback_args",
                    &[
                        (I64, &cb_handle),
                        (DOUBLE, &delay_box),
                        (PTR, &ptr_reg),
                        (I32, &n.to_string()),
                    ],
                );
                return Ok(Some(nanbox_pointer_inline(blk, &id)));
            }
            "setInterval" if args.len() >= 2 => {
                let cb_box = lower_expr(ctx, &args[0])?;
                let delay_box = lower_expr(ctx, &args[1])?;
                let blk = ctx.block();
                let cb_handle = unbox_to_i64(blk, &cb_box);
                if args.len() == 2 {
                    let id = blk.call(
                        I64,
                        "setInterval",
                        &[(I64, &cb_handle), (DOUBLE, &delay_box)],
                    );
                    return Ok(Some(nanbox_pointer_inline(blk, &id)));
                }
                let n = args.len() - 2;
                let buf = ctx.func.alloca_entry_array(DOUBLE, n);
                for (i, a) in args.iter().skip(2).enumerate() {
                    let v = lower_expr(ctx, a)?;
                    let blk = ctx.block();
                    let slot = blk.gep(DOUBLE, &buf, &[(I64, &format!("{}", i))]);
                    blk.store(DOUBLE, &v, &slot);
                }
                let ptr_reg = ctx.block().next_reg();
                ctx.block().emit_raw(format!(
                    "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                    ptr_reg, n, buf
                ));
                let blk = ctx.block();
                let id = blk.call(
                    I64,
                    "js_set_interval_callback_args",
                    &[
                        (I64, &cb_handle),
                        (DOUBLE, &delay_box),
                        (PTR, &ptr_reg),
                        (I32, &n.to_string()),
                    ],
                );
                return Ok(Some(nanbox_pointer_inline(blk, &id)));
            }
            "setImmediate" if !args.is_empty() => {
                let cb_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let cb_handle = unbox_to_i64(blk, &cb_box);
                if args.len() == 1 {
                    let id = blk.call(I64, "js_set_immediate_callback", &[(I64, &cb_handle)]);
                    return Ok(Some(nanbox_pointer_inline(blk, &id)));
                }
                let n = args.len() - 1;
                let buf = ctx.func.alloca_entry_array(DOUBLE, n);
                for (i, a) in args.iter().skip(1).enumerate() {
                    let v = lower_expr(ctx, a)?;
                    let blk = ctx.block();
                    let slot = blk.gep(DOUBLE, &buf, &[(I64, &format!("{}", i))]);
                    blk.store(DOUBLE, &v, &slot);
                }
                let ptr_reg = ctx.block().next_reg();
                ctx.block().emit_raw(format!(
                    "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                    ptr_reg, n, buf
                ));
                let blk = ctx.block();
                let id = blk.call(
                    I64,
                    "js_set_immediate_callback_args",
                    &[(I64, &cb_handle), (PTR, &ptr_reg), (I32, &n.to_string())],
                );
                return Ok(Some(nanbox_pointer_inline(blk, &id)));
            }
            "clearTimeout" | "clearInterval" | "clearImmediate" if !args.is_empty() => {
                let id_box = lower_expr(ctx, &args[0])?;
                let runtime = match property.as_str() {
                    "clearTimeout" => "js_clear_timeout_value",
                    "clearInterval" => "js_clear_interval_value",
                    _ => "js_clear_immediate_value",
                };
                ctx.block().call_void(runtime, &[(DOUBLE, &id_box)]);
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            _ => {}
        }
    }

    if ctx
        .namespace_node_submodules
        .get(ns_name)
        .is_some_and(|submod| submod == "fs/promises")
    {
        match property.as_str() {
            "writeFile" if args.len() >= 2 => {
                let path = lower_expr(ctx, &args[0])?;
                let content = lower_expr(ctx, &args[1])?;
                let options = if args.len() >= 3 {
                    lower_expr(ctx, &args[2])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let promise = ctx.block().call(
                    DOUBLE,
                    "js_fs_promises_write_file",
                    &[(DOUBLE, &path), (DOUBLE, &content), (DOUBLE, &options)],
                );
                return Ok(Some(promise));
            }
            "appendFile" if args.len() >= 2 => {
                let path = lower_expr(ctx, &args[0])?;
                let content = lower_expr(ctx, &args[1])?;
                let options = if args.len() >= 3 {
                    lower_expr(ctx, &args[2])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let promise = ctx.block().call(
                    DOUBLE,
                    "js_fs_promises_append_file",
                    &[(DOUBLE, &path), (DOUBLE, &content), (DOUBLE, &options)],
                );
                return Ok(Some(promise));
            }
            _ => {}
        }
    }
    // Issue #678 followup (namespace branch): wildcard-namespace
    // import to a V8 module — `import * as R from "ramda";
    // R.sum([1,2,3])`. The V8 module has no static export list
    // and (when no companion Named import is present) nothing
    // seeded `import_function_prefixes` for `property`. Route
    // the member call through the bridge using the
    // namespace's specifier before falling through to the
    // native-prefix lookup. Without this, ramda / date-fns /
    // jose / effect wildcard members fell to the
    // `double_literal(0.0)` stub.
    if let Some(specifier) = ctx.namespace_v8_specifiers.get(ns_name).cloned() {
        let mut lowered: Vec<String> = Vec::with_capacity(args.len());
        for a in args {
            lowered.push(lower_expr(ctx, a)?);
        }
        return Ok(Some(crate::expr::emit_v8_export_call(
            ctx, &specifier, property, &lowered,
        )));
    }
    // Issue #680: prefer the per-namespace map so
    // `random.make` and `tracer.make` resolve to their own
    // sources even when both modules export `make`. Falls
    // back to the flat `import_function_prefixes` for
    // namespaces with no overlapping conflicts.
    let Some(source_prefix) = ctx
        .namespace_member_prefixes
        .get(&(ns_name.clone(), property.clone()))
        .cloned()
        .or_else(|| ctx.import_function_prefixes.get(property).cloned())
    else {
        return Ok(None);
    };
    // Issue #678 followup: if the import lands in a V8-fallback
    // module (e.g. `import * as ink from "ink"` where ink fell
    // back to V8 because yoga-layout pulled in a feature Perry
    // can't compile), route the namespace member through the
    // runtime bridge — no `perry_fn_<src>__<member>` symbol
    // exists for the linker to bind to.
    if let Some(specifier) = ctx.import_function_v8_specifiers.get(property).cloned() {
        let mut lowered: Vec<String> = Vec::with_capacity(args.len());
        for a in args {
            lowered.push(lower_expr(ctx, a)?);
        }
        return Ok(Some(crate::expr::emit_v8_export_call(
            ctx, &specifier, property, &lowered,
        )));
    }
    // Issue #678/#5924: re-exported names (e.g. `export { default as
    // render }`) emit `perry_fn_<src>__default` in the origin —
    // resolve the actual origin suffix before forming the symbol.
    // Namespace-scoped lookup first so a rename in a different namespace
    // imported into this file can't clobber this namespace's unrenamed
    // member of the same name.
    let origin_suffix = crate::expr::import_origin_suffix_ns(
        ctx.import_function_origin_names,
        ctx.namespace_member_origin_names,
        ns_name,
        property,
    );
    let symbol = format!("perry_fn_{}__{}", source_prefix, origin_suffix);
    if ctx.imported_vars.contains(property) {
        // Var-shaped export: fetch closure via zero-arg
        // getter, then closure-call with the user args.
        ctx.pending_declares.push((symbol.clone(), DOUBLE, vec![]));
        let closure_box = ctx.block().call(DOUBLE, &symbol, &[]);
        let mut lowered: Vec<String> = Vec::with_capacity(args.len());
        for a in args {
            lowered.push(lower_expr(ctx, a)?);
        }
        if lowered.len() > 16 {
            bail!(
                "perry-codegen: namespace closure call with {} args (max 16)",
                lowered.len()
            );
        }
        let blk = ctx.block();
        let closure_handle = unbox_to_i64(blk, &closure_box);
        let runtime_fn = format!("js_closure_call{}", lowered.len());
        let mut call_args: Vec<(crate::types::LlvmType, &str)> = vec![(I64, &closure_handle)];
        for v in &lowered {
            call_args.push((DOUBLE, v.as_str()));
        }
        return Ok(Some(blk.call(DOUBLE, &runtime_fn, &call_args)));
    }
    // Function-decl-shaped export: direct call with rest bundling.
    let declared_count = ctx
        .imported_func_param_counts
        .get(property)
        .copied()
        .unwrap_or(args.len());
    let has_rest = ctx.imported_func_has_rest.contains(property);
    let mut lowered: Vec<String> = Vec::with_capacity(declared_count);
    if has_rest {
        let fixed_count = declared_count.saturating_sub(1);
        for a in args.iter().take(fixed_count) {
            lowered.push(lower_expr(ctx, a)?);
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
        // Pad missing trailing args with TAG_UNDEFINED.
        let undef_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        while lowered.len() < declared_count {
            lowered.push(undef_lit.clone());
        }
    }
    let arg_types: Vec<crate::types::LlvmType> =
        std::iter::repeat_n(DOUBLE, lowered.len()).collect();
    ctx.pending_declares
        .push((symbol.clone(), DOUBLE, arg_types));
    let arg_slices: Vec<(crate::types::LlvmType, &str)> =
        lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
    Ok(Some(ctx.block().call(DOUBLE, &symbol, &arg_slices)))
}
